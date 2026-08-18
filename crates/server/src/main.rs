//! dllb TCP server.
//!
//! Accepts line-based text queries over TCP, executes them via the
//! query engine, and responds with JSON.

use std::io::{self, Write};
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use dllb_query::{
    ComputeCache, QueryExecutor, SearchServices, WriteVersions, format_error, format_result,
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

    let storage = Arc::new(DllbStorage::open(&db_path).expect("failed to open database"));

    // Process-wide compute cache and write-version map shared across all
    // connection handlers. A cache entry built by one connection is served to
    // all subsequent connections; a RELATE on any connection immediately
    // invalidates the relevant analytics entries.
    let cache = Arc::new(ComputeCache::default());
    let versions = Arc::new(WriteVersions::default());

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

    loop {
        let (stream, addr) = tokio::select! {
            res = listener.accept() => {
                match res {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::error!("accept error: {e}");
                        continue;
                    }
                }
            }
            _ = &mut shutdown_signal => {
                break;
            }
        };
        tracing::info!("connection from {addr}");

        let storage = Arc::clone(&storage);
        let cache = Arc::clone(&cache);
        let versions = Arc::clone(&versions);
        let search = Arc::clone(&search);
        let ns = ns.clone();
        let db = db.clone();

        tokio::spawn(async move {
            let (reader, mut writer) = stream.into_split();
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
                                batch_err =
                                    Some(format_error(&err, dllb_query::OutcomeFormat::Json));
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
            tracing::info!("connection from {addr} closed");
        });
    }
}
