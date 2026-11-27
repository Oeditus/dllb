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

const SCHEMA_NOTE: &str = "\
## Schemas

dllb is **schemaless** by default: any field may be written to any table.

Schemafull tables are enforced by the embedded API
(`Collection::with_schema`), not by the query language -- there is no
`DEFINE TABLE` statement. Use `.indexes` to see what the planner can use.

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

    // Batch mode mirrors the server's `BEGIN BATCH` / `END BATCH` line
    // protocol so the REPL can drive bulk loads interactively.
    let mut batch: Option<Vec<String>> = None;

    loop {
        let prompt = if batch.is_some() {
            "dllb[batch]> "
        } else {
            "dllb> "
        };

        match rl.readline(prompt) {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                rl.add_history_entry(trimmed).ok();

                // -- Batch accumulation ------------------------------------
                if let Some(stmts) = batch.as_mut() {
                    if trimmed.eq_ignore_ascii_case("END BATCH") {
                        let collected = batch.take().unwrap_or_default();
                        run_batch(&executor, &collected, &r);
                        continue;
                    }
                    stmts.push(trimmed.trim_end_matches(';').to_string());
                    continue;
                }
                if trimmed.eq_ignore_ascii_case("BEGIN BATCH") {
                    batch = Some(Vec::new());
                    println!("{}", r.md("*Batch started. Type `END BATCH` to apply.*"));
                    continue;
                }

                match trimmed {
                    ".quit" | ".exit" => break,
                    ".help" => {
                        println!("{}", r.md(HELP));
                        continue;
                    }
                    ".indexes" => {
                        print_indexes(&executor, &r);
                        continue;
                    }
                    ".schema" => {
                        println!("{}", r.md(SCHEMA_NOTE));
                        continue;
                    }
                    other if other.starts_with('.') => {
                        eprintln!("dllb: unknown command '{other}'. Try `.help` for the list.");
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
                        // Honour the requested OUTCOME even on failure, so a
                        // CSV/TOON user does not get a JSON error body.
                        let outcome = dllb_query::parse(query)
                            .map(|q| q.outcome)
                            .unwrap_or(OutcomeFormat::Json);
                        eprintln!(
                            "{}",
                            r.error(&format_error(&err, outcome), outcome_lang(outcome))
                        );
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl-C abandons an in-progress batch instead of the shell.
                if batch.take().is_some() {
                    println!("{}", r.md("*Batch discarded.*"));
                } else {
                    break;
                }
            }
            Err(ReadlineError::Eof) => break,
            Err(err) => {
                eprintln!("{}", r.error(&format!(r#"{{"error":"{err}"}}"#), "json",));
                break;
            }
        }
    }

    if batch.is_some() {
        println!("{}", r.md("*Unfinished batch discarded.*"));
    }

    rl.save_history(".dllb_history").ok();
    println!("{}", r.md("*Bye.*"));
}

/// Apply a collected `BEGIN BATCH ... END BATCH` block.
fn run_batch(executor: &QueryExecutor<'_>, lines: &[String], r: &render::Renderer) {
    let mut stmts = Vec::with_capacity(lines.len());
    for line in lines {
        match dllb_query::parse(line) {
            Ok(q) => stmts.push(q.statement),
            Err(err) => {
                eprintln!(
                    "{}",
                    r.error(&format_error(&err, OutcomeFormat::Json), "json")
                );
                return;
            }
        }
    }

    match executor.execute_batch(&stmts) {
        Ok(result) => println!(
            "{}",
            r.code(&format_result(&result, OutcomeFormat::Json), "json")
        ),
        Err(err) => eprintln!(
            "{}",
            r.error(&format_error(&err, OutcomeFormat::Json), "json")
        ),
    }
}

/// Print the index catalog for the current namespace/database.
///
/// There is no `SHOW INDEXES` statement in the language, so the CLI reads the
/// catalog directly from storage and renders it as a Markdown table rather
/// than raw JSON -- this is a discovery aid, not machine output.
fn print_indexes(executor: &QueryExecutor<'_>, r: &render::Renderer) {
    let rows = match executor.describe_indexes() {
        Ok(rows) => rows,
        Err(err) => {
            eprintln!(
                "{}",
                r.error(&format_error(&err, OutcomeFormat::Json), "json")
            );
            return;
        }
    };

    if rows.is_empty() {
        println!("{}", r.md("*No indexes defined.*"));
        return;
    }

    let mut md = String::from("| Table | Index | Kind | Fields | Unique |\n");
    md.push_str("|-------|-------|------|--------|--------|\n");
    for row in &rows {
        let cell = |key: &str| match row.get(key) {
            Some(v) => v.to_string(),
            None => String::new(),
        };
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            cell("table"),
            cell("name"),
            cell("kind"),
            cell("fields"),
            cell("unique"),
        ));
    }
    println!("{}", r.md(&md));
}

fn outcome_lang(outcome: OutcomeFormat) -> &'static str {
    match outcome {
        OutcomeFormat::Json => "json",
        OutcomeFormat::Toon => "toml",
        OutcomeFormat::Csv => "csv",
    }
}
