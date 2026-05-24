# Server and CLI

This document covers the `dllb-server` TCP server and `dllb-cli`
interactive REPL.

## TCP Server (`dllb-server`)

Line-based text protocol over TCP using tokio:

1. Client connects to the server (default `127.0.0.1:9800`)
2. Sends one query per line (terminated by `\n`)
3. Server parses and executes via `QueryExecutor::run()`
4. Server responds with a JSON line

### Running

```bash
cargo run -p dllb-server
```

### Configuration (environment variables)

| Variable | Default | Description |
|----------|---------|-------------|
| `DLLB_BIND` | `127.0.0.1:9800` | Bind address |
| `DLLB_PATH` | `dllb.redb` | Database file path |
| `DLLB_NS` | `default` | Default namespace |
| `DLLB_DB` | `default` | Default database |

### Protocol

Each response is a single JSON line:

```json
{"status":"created","id":"user:alice"}
{"status":"rows","count":2,"data":[{"id":"user:alice","name":"Alice"},{"id":"user:bob","name":"Bob"}]}
{"status":"deleted","existed":true}
{"status":"ok"}
{"status":"error","message":"expected identifier, got ..."}
```

### Example session (via netcat)

```bash
$ nc localhost 9800
CREATE user:alice SET name = 'Alice', age = 30
{"status":"created","id":"user:alice"}
SELECT * FROM user
{"status":"rows","count":1,"data":[{"age":30,"id":"user:alice","name":"Alice"}]}
DELETE user:alice
{"status":"deleted","existed":true}
```

## CLI REPL (`dllb-cli`)

Interactive shell with line editing (rustyline), command history, and
embedded database access (no network).

### Running

```bash
cargo run -p dllb-cli
```

### Options

```
dllb-cli [OPTIONS]
  --path <PATH>   Database file path (default: ./dllb.redb)
  --ns <NS>       Namespace (default: default)
  --db <DB>       Database (default: default)
```

### Commands

| Command | Description |
|---------|-------------|
| `.quit` / `.exit` | Exit the shell |
| `.help` | Show available commands and query syntax |

### Example session

```
dllb v0.2.0 -- interactive shell
Database: dllb.redb  Namespace: default  Database: default
Type a query, .help for commands, .quit to exit.

dllb> CREATE user:alice SET name = 'Alice', age = 30;
{"status":"created","id":"user:alice"}
dllb> SELECT * FROM user;
{"status":"rows","count":1,"data":[{"age":30,"id":"user:alice","name":"Alice"}]}
dllb> .quit
Bye.
```

Command history is saved to `.dllb_history` in the current directory.

## Response Format

Both server and CLI use the same JSON format from
`dllb_query::format_result()`:

| QueryResult variant | JSON |
|--------------------|------|
| `Ok` | `{"status":"ok"}` |
| `Created { id }` | `{"status":"created","id":"table:id"}` |
| `Deleted { existed }` | `{"status":"deleted","existed":true/false}` |
| `Rows(data)` | `{"status":"rows","count":N,"data":[...]}` |
| Error | `{"status":"error","message":"..."}` |
