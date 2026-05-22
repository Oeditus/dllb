//! dllb server binary.

fn main() {
    tracing_subscriber::fmt::init();
    tracing::info!("dllb server starting...");

    // TODO: Initialize storage, query engine, and TCP listener.
    println!("dllb v{}", env!("CARGO_PKG_VERSION"));
}
