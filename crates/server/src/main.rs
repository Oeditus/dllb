//! dllb TCP server.
//!
//! Accepts line-based text queries over TCP, executes them via the
//! query engine, and responds with JSON.

use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use dllb_query::{QueryExecutor, format_error, format_result};
use dllb_storage::db::DllbStorage;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let bind = std::env::var("DLLB_BIND").unwrap_or_else(|_| "127.0.0.1:9800".into());
    let db_path = std::env::var("DLLB_PATH").unwrap_or_else(|_| "dllb.redb".into());
    let ns = std::env::var("DLLB_NS").unwrap_or_else(|_| "default".into());
    let db = std::env::var("DLLB_DB").unwrap_or_else(|_| "default".into());

    let storage = Arc::new(DllbStorage::open(&db_path).expect("failed to open database"));

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

                let executor = QueryExecutor::new(&storage, &ns, &db);
                let response = match executor.run(query) {
                    Ok(result) => format_result(&result),
                    Err(err) => format_error(&err),
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
