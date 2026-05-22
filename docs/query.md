# Query Engine

This document covers the `dllb-query` crate: the tokenizer, parser, AST,
and executor for the dllb query language.

## Overview

The query engine provides a SQL-like declarative language inspired by
SurrealQL. In this prototype, a minimal subset is implemented:

| Statement | Syntax | Description |
|-----------|--------|-------------|
| CREATE | `CREATE table[:id] SET field = value, ...;` | Insert a document |
| SELECT | `SELECT */fields FROM table[:id] [WHERE field = value];` | Query documents |
| DELETE | `DELETE table:id;` | Remove a document |
| RELATE | `RELATE src->edge_type->dst [SET field = value, ...];` | Create a graph edge |

## Architecture

```
Input string
    |
    v
[Tokenizer] -> Vec<Token>
    |
    v
[Parser] -> Statement (AST)
    |
    v
[Executor] -> QueryResult
```

No optimizer or planner yet -- the executor dispatches directly to the
`Collection` and `EdgeStore` APIs.

## Tokenizer

Splits input into tokens:

- **Keywords**: CREATE, SELECT, DELETE, RELATE, FROM, WHERE, SET, AND
  (case-insensitive)
- **Identifiers**: `[a-zA-Z_][a-zA-Z0-9_]*`
- **String literals**: `'single-quoted'`
- **Numbers**: integer (`42`) or float (`3.14`)
- **Booleans**: `true`, `false`
- **Symbols**: `->`, `=`, `,`, `;`, `:`, `*`, `.`
- **Comments**: `-- line comments` (skipped)

## Parser

Hand-written recursive descent. No external parser crate.

```rust
let stmt = dllb_query::parse("SELECT * FROM user WHERE age = 30;")?;
```

Produces a `Statement` AST node.

## Executor

Maps AST nodes to concrete crate API calls:

```rust
let executor = QueryExecutor::new(&storage, "ns", "db");

// Parse + execute in one call:
let result = executor.run("CREATE user:alice SET name = 'Alice';")?;

// Or parse separately:
let stmt = dllb_query::parse("SELECT * FROM user;")?;
let result = executor.execute(&stmt)?;
```

### QueryResult

```rust
pub enum QueryResult {
    Ok,                                           // RELATE
    Created { id: RecordId },                     // CREATE
    Deleted { existed: bool },                    // DELETE
    Rows(Vec<BTreeMap<String, Value>>),          // SELECT
}
```

### Execution Mapping

| Statement | Crate API |
|-----------|-----------|
| CREATE | `Collection::create_with_id()` |
| SELECT (table) | `Collection::scan_all()` + in-memory filter |
| SELECT (record) | `Collection::get()` |
| DELETE | `Collection::delete()` |
| RELATE | `EdgeStore::relate()` |

## Examples

```sql
-- Create documents
CREATE user:alice SET name = 'Alice', age = 30;
CREATE user:bob SET name = 'Bob', age = 25;

-- Query all
SELECT * FROM user;

-- Point lookup
SELECT name FROM user:alice;

-- Filtered scan
SELECT * FROM user WHERE age = 30;

-- Delete
DELETE user:alice;

-- Create graph edge
RELATE user:alice->knows->user:bob SET since = 2020;
```

## Future Extensions

- UPDATE/MERGE statements
- ORDER BY, LIMIT, GROUP BY
- Graph traversal in SELECT (`->edge->` syntax)
- Full-text `@@` operator
- Vector KNN `<|K,ef|>` operator
- Expressions and functions
- Query optimizer with index selection

## Testing

```bash
cargo test -p dllb-query
```

22 tests: 11 unit (tokenizer + parser) + 11 integration (end-to-end
CREATE/SELECT/DELETE/RELATE/WHERE through the executor).
