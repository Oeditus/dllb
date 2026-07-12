//! dllb interactive REPL.
//!
//! Opens an embedded database and provides a line-editing shell for
//! executing queries. Results are rendered with syntax highlighting
//! via [`marcli`].

mod render;

use dllb_query::{
    ComputeCache, OutcomeFormat, QueryExecutor, SearchServices, WriteVersions, format_error,
    format_result,
};
use dllb_storage::db::DllbStorage;
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::sync::Arc;

const HELP: &str = "\
## Commands

| Command | Description |
|---------|-------------|
| `.quit` / `.exit` | Exit the shell |
| `.help` | Show this help |

## Queries

```sql
CREATE table:id SET field = value, ...;
SELECT */fields FROM table[:id] [WHERE field = value];
DELETE table:id;
RELATE src->edge->dst [SET field = value, ...];
```
";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let db_path = get_arg(&args, "--path").unwrap_or_else(|| "dllb.redb".into());
    let ns = get_arg(&args, "--ns").unwrap_or_else(|| "default".into());
    let db = get_arg(&args, "--db").unwrap_or_else(|| "default".into());

    let r = if has_flag(&args, "--no-color") {
        render::Renderer::plain()
    } else {
        render::Renderer::colored()
    };

    let banner = format!(
        "# dllb v{ver}\n\n\
         `{db_path}` | namespace **{ns}** | database **{db}**\n\n\
         Type a query, `.help` for commands, `.quit` to exit.",
        ver = env!("CARGO_PKG_VERSION"),
    );
    println!("{}\n", r.md(&banner));

    let storage = DllbStorage::open(&db_path).expect("failed to open database");

    // Enable process-wide cache, versioning, and full-text/vector search services in the CLI.
    let cache = Arc::new(ComputeCache::default());
    let versions = Arc::new(WriteVersions::default());
    let search = Arc::new(SearchServices::new(format!("{db_path}.search")));

    let executor = QueryExecutor::new_with_services(&storage, &ns, &db, cache, versions, search);

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
                        println!("{}", r.md(HELP));
                        continue;
                    }
                    _ => {}
                }

                let query = trimmed.trim_end_matches(';').trim();
                match executor.run(query) {
                    Ok((result, outcome)) => {
                        let lang = outcome_lang(outcome);
                        println!("{}", r.code(&format_result(&result, outcome), lang));
                    }
                    Err(err) => {
                        eprintln!(
                            "{}",
                            r.error(&format_error(&err, OutcomeFormat::Json), "json",)
                        );
                    }
                }
            }
            Err(ReadlineError::Interrupted | ReadlineError::Eof) => break,
            Err(err) => {
                eprintln!("{}", r.error(&format!(r#"{{"error":"{err}"}}"#), "json",));
                break;
            }
        }
    }

    rl.save_history(".dllb_history").ok();
    println!("{}", r.md("*Bye.*"));
}

fn get_arg(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

fn outcome_lang(outcome: OutcomeFormat) -> &'static str {
    match outcome {
        OutcomeFormat::Json => "json",
        OutcomeFormat::Toon => "toml",
        OutcomeFormat::Csv => "csv",
    }
}
