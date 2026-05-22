//! dllb interactive REPL.
//!
//! Opens an embedded database and provides a line-editing shell for
//! executing queries. Results are printed as formatted JSON.

use dllb_query::{QueryExecutor, format_error, format_result};
use dllb_storage::db::DllbStorage;
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let db_path = get_arg(&args, "--path").unwrap_or_else(|| "dllb.redb".into());
    let ns = get_arg(&args, "--ns").unwrap_or_else(|| "default".into());
    let db = get_arg(&args, "--db").unwrap_or_else(|| "default".into());

    println!("dllb v{} -- interactive shell", env!("CARGO_PKG_VERSION"));
    println!("Database: {db_path}  Namespace: {ns}  Database: {db}");
    println!("Type a query, .help for commands, .quit to exit.\n");

    let storage = DllbStorage::open(&db_path).expect("failed to open database");
    let executor = QueryExecutor::new(&storage, &ns, &db);

    let mut rl = DefaultEditor::new().expect("failed to create editor");
    let _ = rl.load_history(".dllb_history");

    loop {
        match rl.readline("dllb> ") {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                rl.add_history_entry(trimmed).ok();

                match trimmed {
                    ".quit" | ".exit" => break,
                    ".help" => {
                        println!("Commands:");
                        println!("  .quit / .exit  Exit the shell");
                        println!("  .help          Show this help");
                        println!();
                        println!("Queries:");
                        println!("  CREATE table:id SET field = value, ...;");
                        println!("  SELECT */fields FROM table[:id] [WHERE field = value];");
                        println!("  DELETE table:id;");
                        println!("  RELATE src->edge->dst [SET field = value, ...];");
                        continue;
                    }
                    _ => {}
                }

                let query = trimmed.trim_end_matches(';').trim();
                match executor.run(query) {
                    Ok(result) => println!("{}", format_result(&result)),
                    Err(err) => println!("{}", format_error(&err)),
                }
            }
            Err(ReadlineError::Interrupted | ReadlineError::Eof) => break,
            Err(err) => {
                eprintln!("Error: {err}");
                break;
            }
        }
    }

    rl.save_history(".dllb_history").ok();
    println!("Bye.");
}

fn get_arg(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}
