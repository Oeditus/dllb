//! dllb interactive REPL.

fn main() {
    tracing_subscriber::fmt::init();
    println!("dllb v{} -- interactive shell", env!("CARGO_PKG_VERSION"));
    println!("Type a query or 'quit' to exit.\n");

    // TODO: Initialize embedded database and REPL loop (rustyline).
}
