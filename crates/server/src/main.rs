//! dllb TCP server.
//!
//! Accepts line-based text queries over TCP, executes them via the
//! query engine, and responds with JSON.

use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use dllb_query::{
    ComputeCache, QueryExecutor, SearchServices, WriteVersions, format_error, format_result,
};
use dllb_storage::db::DllbStorage;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

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

    let listener = TcpListener::bind(&bind)
        .await
        .unwrap_or_else(|e| panic!("failed to bind to {bind}: {e}"));

    tracing::info!("dllb v{} listening on {bind}", env!("CARGO_PKG_VERSION"));
    println!("dllb v{} listening on {bind}", env!("CARGO_PKG_VERSION"));

    loop {
        let (stream, addr) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("accept error: {e}");
                continue;
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
