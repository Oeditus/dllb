//! dllb TCP and Unix domain socket server.
//!
//! Accepts line-based text queries over TCP or Unix domain sockets,
//! executes them via the query engine, and responds with JSON.

use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use dllb_query::{
    ComputeCache, QueryExecutor, SearchServices, format_error, format_result, global_write_versions,
};
use dllb_storage::db::DllbStorage;

#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

/// A writer wrapper that swallows `BrokenPipe` errors.
///
/// When running `dllb-server` in non-interactive or child-process environments (such as
/// external test runners like Elixir's `Port` in `ragex`), stdout or stderr pipes can be
/// closed when the test harness or client shuts down.
///
/// Standard `tracing-subscriber` writers attempt `write_all(...)` and print noisy error
/// messages to stderr when stdout/stderr returns `ErrorKind::BrokenPipe`. Returning
/// `Ok(buf.len())` / `Ok(())` on `BrokenPipe` ensures log events are cleanly discarded when output pipes close.
pub struct SafeWriter<W> {
    inner: W,
}

impl<W> SafeWriter<W> {
    pub fn new(inner: W) -> Self {
        Self { inner }
    }
}

impl<W: Write> Write for SafeWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.inner.write(buf) {
            Ok(n) => Ok(n),
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(buf.len()),
            Err(e) => Err(e),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.inner.flush() {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
            Err(e) => Err(e),
        }
    }
}

/// Open the database at `db_path` or exit/panic with comprehensive diagnostic details.
fn open_storage_or_panic(db_path: &str) -> Arc<DllbStorage> {
    match DllbStorage::open(db_path) {
        Ok(storage) => Arc::new(storage),
        Err(err) => {
            let path = Path::new(db_path);
            let canonical = path.canonicalize().unwrap_or_else(|_| {
                std::env::current_dir()
                    .map(|cwd| cwd.join(path))
                    .unwrap_or_else(|_| path.to_path_buf())
            });

            let lock_file = canonical.display().to_string();
            let exists = path.exists();
            let file_details = if exists {
                match std::fs::metadata(path) {
                    Ok(meta) => format!(
                        "Exists ({} bytes, modified {:?})",
                        meta.len(),
                        meta.modified().ok()
                    ),
                    Err(e) => format!("Exists (failed to read metadata: {e})"),
                }
            } else {
                "Does not exist (creation attempted)".to_string()
            };

            let search_dir = format!("{db_path}.search");
            let search_exists = Path::new(&search_dir).exists();

            let diagnostic = format!(
                "\n\
================================================================================\n\
DATABASE OPEN FAILURE DIAGNOSTICS\n\
================================================================================\n\
What was tried : Attempted to open/create redb database at '{db_path}' with exclusive lock.\n\
What failed    : {err}\n\
Lock Target    : {lock_file}\n\
Database File  : {db_path} [{file_details}]\n\
Search Index   : {search_dir} [Exists: {search_exists}]\n\
--------------------------------------------------------------------------------\n\
Possible Causes & Troubleshooting:\n\
  1. Another process (`dllb-server` or `dllb` CLI) is already running and holding\n\
     the lock on database file '{lock_file}'.\n\
  2. If spawning multiple DB instances concurrently, ensure each instance uses\n\
     a unique database path via DLLB_PATH (e.g. DLLB_PATH=dllb_3009.redb).\n\
  3. To check which process holds the file lock, run:\n\
     $ lsof \"{lock_file}\"   OR   $ fuser \"{lock_file}\"\n\
================================================================================\n"
            );

            eprintln!("{diagnostic}");
            panic!("failed to open database: {err}");
        }
    }
}

enum ConnectionStream {
    Tcp(tokio::net::TcpStream),
    #[cfg(unix)]
    Unix(tokio::net::UnixStream),
}

/// Process-wide state shared across all connection handlers.
struct ServerState {
    storage: Arc<DllbStorage>,
    cache: Arc<ComputeCache>,
    versions: Arc<dllb_graph::WriteVersions>,
    search: Arc<SearchServices>,
    ns: String,
    db: String,
}

#[tokio::main]
async fn main() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .or_else(|_| {
            tracing_subscriber::EnvFilter::try_new(
                std::env::var("DLLB_LOG").unwrap_or_else(|_| "info".into()),
            )
        })
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(|| SafeWriter::new(io::stdout()))
        .init();

    let bind = std::env::var("DLLB_BIND").unwrap_or_else(|_| "127.0.0.1:3009".into());
    let db_path = std::env::var("DLLB_PATH").unwrap_or_else(|_| "dllb.redb".into());
    let ns = std::env::var("DLLB_NS").unwrap_or_else(|_| "default".into());
    let db = std::env::var("DLLB_DB").unwrap_or_else(|_| "default".into());

    let storage = open_storage_or_panic(&db_path);

    // Process-wide compute cache and write-version map shared across all
    // connection handlers. A cache entry built by one connection is served to
    // all subsequent connections. The version map is the same registry that
    // `EdgeStore` bumps, so any edge mutation -- from any connection --
    // invalidates the relevant analytics entries.
    let cache = Arc::new(ComputeCache::default());
    let versions = global_write_versions();

    // Process-wide full-text/vector index services, shared across all
    // connection handlers. Tantivy indexes live on disk beside the database
    // file; HNSW vector indexes are held in memory and rebuilt on first use.
    let search = Arc::new(SearchServices::new(format!("{db_path}.search")));

    let addr: std::net::SocketAddr = bind
        .parse()
        .unwrap_or_else(|e| panic!("invalid bind address '{bind}': {e}"));

    let socket = if addr.is_ipv6() {
        tokio::net::TcpSocket::new_v6()
    } else {
        tokio::net::TcpSocket::new_v4()
    }
    .unwrap_or_else(|e| panic!("failed to create socket for {bind}: {e}"));

    socket.set_reuseaddr(true).ok();
    #[cfg(not(windows))]
    socket.set_reuseport(true).ok();

    socket
        .bind(addr)
        .unwrap_or_else(|e| panic!("failed to bind to {bind}: {e}"));

    let listener = socket
        .listen(1024)
        .unwrap_or_else(|e| panic!("failed to listen on {bind}: {e}"));

    let bound_addr = listener.local_addr().unwrap_or(addr);
    let port = bound_addr.port();

    let sock_path = match std::env::var("DLLB_SOCK") {
        Ok(s) => {
            if s.contains("{port}") {
                s.replace("{port}", &port.to_string())
            } else if s.ends_with(".sock") && !s.contains(&port.to_string()) {
                let stem = s.trim_end_matches(".sock");
                format!("{stem}_{port}.sock")
            } else {
                s
            }
        }
        Err(_) => format!("dllb_{port}.sock"),
    };

    #[cfg(unix)]
    let unix_listener = {
        if let Some(parent) = Path::new(&sock_path)
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::remove_file(&sock_path);
        match tokio::net::UnixListener::bind(&sock_path) {
            Ok(l) => {
                tracing::info!("dllb Unix socket listening on {sock_path}");
                let _ = writeln!(
                    SafeWriter::new(io::stdout()),
                    "dllb Unix socket listening on {sock_path}"
                );
                Some(l)
            }
            Err(e) => {
                tracing::warn!("failed to bind Unix socket on {sock_path}: {e}");
                None
            }
        }
    };

    tracing::info!("dllb v{} listening on {bind}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        SafeWriter::new(io::stdout()),
        "dllb v{} listening on {bind}",
        env!("CARGO_PKG_VERSION")
    );

    let shutdown_signal = async {
        let watch_stdin = match std::env::var("DLLB_WATCH_STDIN")
            .or_else(|_| std::env::var("DLLB_EXIT_ON_STDIN"))
            .as_deref()
        {
            Ok("1") | Ok("true") | Ok("yes") => true,
            Ok("0") | Ok("false") | Ok("no") => false,
            _ => {
                use std::io::IsTerminal;
                !std::io::stdin().is_terminal()
            }
        };

        let stdin_fut = async move {
            if !watch_stdin {
                std::future::pending::<()>().await;
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            use tokio::io::AsyncReadExt;
            let mut stdin = tokio::io::stdin();
            let mut buf = [0u8; 1];
            loop {
                match stdin.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }
        };

        #[cfg(unix)]
        let terminate_fut = async {
            use tokio::signal::unix::{SignalKind, signal};
            if let Ok(mut sig) = signal(SignalKind::terminate()) {
                sig.recv().await;
            } else {
                std::future::pending::<()>().await;
            }
        };

        #[cfg(not(unix))]
        let terminate_fut = std::future::pending::<()>();

        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate_fut => {},
            _ = stdin_fut => {},
        }
        tracing::info!("Shutdown signal received, stopping server");
    };

    tokio::pin!(shutdown_signal);

    let state = Arc::new(ServerState {
        storage,
        cache,
        versions,
        search,
        ns,
        db,
    });

    loop {
        #[cfg(unix)]
        let stream_res = {
            if let Some(ref ul) = unix_listener {
                tokio::select! {
                    res = listener.accept() => res.map(|(s, a)| (ConnectionStream::Tcp(s), a.to_string())),
                    res = ul.accept() => res.map(|(s, _a)| (ConnectionStream::Unix(s), "unix".to_string())),
                    _ = &mut shutdown_signal => break,
                }
            } else {
                tokio::select! {
                    res = listener.accept() => res.map(|(s, a)| (ConnectionStream::Tcp(s), a.to_string())),
                    _ = &mut shutdown_signal => break,
                }
            }
        };

        #[cfg(not(unix))]
        let stream_res = tokio::select! {
            res = listener.accept() => res.map(|(s, a)| (ConnectionStream::Tcp(s), a.to_string())),
            _ = &mut shutdown_signal => break,
        };

        let (conn, addr_str) = match stream_res {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("accept error: {e}");
                continue;
            }
        };
        tracing::info!("connection from {addr_str}");

        let state = Arc::clone(&state);

        tokio::spawn(async move {
            match conn {
                ConnectionStream::Tcp(stream) => {
                    let (reader, writer) = stream.into_split();
                    handle_connection(reader, writer, addr_str.clone(), state).await;
                }
                #[cfg(unix)]
                ConnectionStream::Unix(stream) => {
                    let (reader, writer) = stream.into_split();
                    handle_connection(reader, writer, addr_str.clone(), state).await;
                }
            }
            tracing::info!("connection from {addr_str} closed");
        });
    }

    #[cfg(unix)]
    let _ = std::fs::remove_file(&sock_path);
}

async fn handle_connection<R, W>(
    reader: R,
    mut writer: W,
    remote_addr: String,
    state: Arc<ServerState>,
) where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let storage = Arc::clone(&state.storage);
    let cache = Arc::clone(&state.cache);
    let versions = Arc::clone(&state.versions);
    let search = Arc::clone(&state.search);
    let ns = state.ns.clone();
    let db = state.db.clone();

    let mut lines = BufReader::new(reader).lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let query = line.trim().trim_end_matches(';').trim();
        if query.is_empty() {
            continue;
        }

        // --- BEGIN BATCH / END BATCH protocol ---
        if query.eq_ignore_ascii_case("BEGIN BATCH") {
            let mut batch_stmts: Vec<dllb_query::ast::Statement> = Vec::new();
            let mut batch_err: Option<String> = None;

            while let Ok(Some(batch_line)) = lines.next_line().await {
                let bq = batch_line.trim().trim_end_matches(';').trim();
                if bq.eq_ignore_ascii_case("END BATCH") {
                    break;
                }
                if bq.is_empty() {
                    continue;
                }
                if batch_err.is_some() {
                    // Already failed -- drain until END BATCH.
                    continue;
                }
                match dllb_query::parse(bq) {
                    Ok(q) => batch_stmts.push(q.statement),
                    Err(err) => {
                        batch_err = Some(format_error(&err, dllb_query::OutcomeFormat::Json));
                    }
                }
            }

            let response = if let Some(err_resp) = batch_err {
                err_resp
            } else {
                let executor = QueryExecutor::new_with_services(
                    &storage,
                    &ns,
                    &db,
                    Arc::clone(&cache),
                    Arc::clone(&versions),
                    Arc::clone(&search),
                );
                match executor.execute_batch(&batch_stmts) {
                    Ok(result) => format_result(&result, dllb_query::OutcomeFormat::Json),
                    Err(err) => format_error(&err, dllb_query::OutcomeFormat::Json),
                }
            };

            if writer
                .write_all(format!("{response}\n").as_bytes())
                .await
                .is_err()
            {
                break;
            }
            continue;
        }

        let executor = QueryExecutor::new_with_services(
            &storage,
            &ns,
            &db,
            Arc::clone(&cache),
            Arc::clone(&versions),
            Arc::clone(&search),
        );
        let response = match executor.run(query) {
            Ok((result, outcome)) => format_result(&result, outcome),
            Err(err) => format_error(&err, dllb_query::OutcomeFormat::Json),
        };

        if writer
            .write_all(format!("{response}\n").as_bytes())
            .await
            .is_err()
        {
            break;
        }
    }
    tracing::info!("connection from {remote_addr} handler finished");
}
