//! dllb interactive REPL.
//!
//! Opens an embedded database and provides a line-editing shell for
//! executing queries. Results are rendered with syntax highlighting
//! via [`marcli`].

mod render;

use dllb_query::{
    ComputeCache, OutcomeFormat, QueryExecutor, SearchServices, format_error, format_result,
    global_write_versions,
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

## Flags

| Flag | Description |
|------|-------------|
| `--path <file>` | Database file to open (default `dllb.redb`) |
| `--ns <name>` | Namespace (default `default`) |
| `--db <name>` | Database (default `default`) |
| `--no-color` | Disable ANSI colours |
| `-h`, `--help` | Show this help and exit |
| `-V`, `--version` | Print the version and exit |

## Queries

```sql
CREATE table[:id] SET field = value, ... [ON CONFLICT UPDATE [SET ...]];
SELECT */fields FROM table[:id] [WHERE ...] [ORDER BY f [DESC]] [LIMIT n];
UPDATE table[:id] SET field = value, ... [WHERE ...];
DELETE table:id;
DELETE table WHERE ...;
DELETE ALL FROM table;
COUNT table [WHERE ...] [GROUP BY field];

RELATE src->edge->dst [SET field = value, ...];

DEFINE INDEX name ON table FIELDS f[, ...] [UNIQUE];
DEFINE FULLTEXT INDEX name ON table FIELDS f [ANALYZER name];
DEFINE VECTOR INDEX name ON table FIELDS f DIMENSION n [METRIC m];
REMOVE INDEX name ON table;
COMPACT;

SEARCH table field 'query' [WHERE ...] [LIMIT n];
VECTOR SEARCH table field [v1, ...] [WHERE ...] [K n];
HYBRID SEARCH table TEXT f 'query' VECTOR f [v] [ALPHA a] [LIMIT n];

GRAPH COMMUNITIES table [ALGORITHM louvain|lp] [MAX_ITER n] [RESOLUTION f];
GRAPH COMPONENTS table;
GRAPH PAGERANK table [DAMPING f] [MAX_ITER n] [LIMIT n];
GRAPH CENTRALITY table [DEGREE|INDEGREE|OUTDEGREE] [LIMIT n];
GRAPH PATH src -> dst ON table [MAX_DEPTH n];
GRAPH EDGES table [WHERE ...];
```

Append `OUTCOME JSON|TOON|CSV` to any query to change the wire format.
";

fn usage_error(message: &str) -> ! {
    eprintln!("dllb: {message}\n");
    eprintln!("Try `dllb-cli --help` for usage.");
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Handle informational and unknown flags before touching the database:
    // silently ignoring a typo like `--pth` opens the wrong file and is very
    // hard to diagnose from inside the REPL.
    let mut db_path = "dllb.redb".to_string();
    let mut ns = "default".to_string();
    let mut db = "default".to_string();
    let mut no_color = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("{}", render::Renderer::plain().md(HELP));
                return;
            }
            "-V" | "--version" => {
                println!("dllb-cli {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--no-color" => {
                no_color = true;
                i += 1;
            }
            flag @ ("--path" | "--ns" | "--db") => {
                let Some(value) = args.get(i + 1) else {
                    usage_error(&format!("{flag} requires a value"));
                };
                match flag {
                    "--path" => db_path = value.clone(),
                    "--ns" => ns = value.clone(),
                    _ => db = value.clone(),
                }
                i += 2;
            }
            other => usage_error(&format!("unknown argument '{other}'")),
        }
    }

    let r = if no_color {
        render::Renderer::plain()
    } else {
        render::Renderer::colored()
    };

    let banner = format!(
        "# dllb v{ver}\n\n\
         `{db_path}` | namespace **{ns}** | database **{db}**\n\n\
         Embedded mode: this shell opens the database file directly, so\n\
         `dllb-server` must not be holding the same `{db_path}`.\n\n\
         Type a query, `.help` for commands, `.quit` to exit.",
        ver = env!("CARGO_PKG_VERSION"),
    );
    println!("{}\n", r.md(&banner));

    let storage = match DllbStorage::open(&db_path) {
        Ok(storage) => storage,
        Err(err) => {
            eprintln!("dllb: cannot open '{db_path}': {err}");
            eprintln!(
                "hint: another process (dllb-server or dllb-cli) may hold the lock; \
                 run `lsof {db_path}` to check."
            );
            std::process::exit(1);
        }
    };

    // Enable process-wide cache, versioning, and full-text/vector search services in the CLI.
    let cache = Arc::new(ComputeCache::default());
    let versions = global_write_versions();
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

fn outcome_lang(outcome: OutcomeFormat) -> &'static str {
    match outcome {
        OutcomeFormat::Json => "json",
        OutcomeFormat::Toon => "toml",
        OutcomeFormat::Csv => "csv",
    }
}
