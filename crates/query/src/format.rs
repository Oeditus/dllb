//! Output formatting for [`QueryResult`] values.
//!
//! Supports three wire formats selected by `OUTCOME`:
//! - **JSON** (default) -- standard JSON objects
//! - **TOON** -- TOML Object Notation
//! - **CSV** -- comma-separated values
//!
//! Reused by both `dllb-server` and `dllb-cli`.

use std::collections::BTreeMap;

use dllb_core::Value;

use crate::ast::OutcomeFormat;
use crate::executor::QueryResult;

/// Format a successful result in the requested format.
pub fn format_result(result: &QueryResult, outcome: OutcomeFormat) -> String {
    match outcome {
        OutcomeFormat::Json => format_result_json(result),
        OutcomeFormat::Toon => format_result_toon(result),
        OutcomeFormat::Csv => format_result_csv(result),
    }
}

/// Format an error in the requested format.
pub fn format_error(err: &dllb_core::Error, outcome: OutcomeFormat) -> String {
    match outcome {
        OutcomeFormat::Json => format_error_json(err),
        OutcomeFormat::Toon => format_error_toon(err),
        OutcomeFormat::Csv => format_error_csv(err),
    }
}

// ---------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------

fn format_result_json(result: &QueryResult) -> String {
    match result {
        QueryResult::CachedResponse(s) => s.clone(),
        QueryResult::Ok => r#"{"status":"ok"}"#.to_string(),
        QueryResult::Created { id } => {
            let id_json = serde_json::to_string(&id.to_string()).unwrap_or_else(|_| "\"\"".into());
            format!(r#"{{"status":"created","id":{id_json}}}"#)
        }
        QueryResult::Updated { id } => {
            let id_json = serde_json::to_string(&id.to_string()).unwrap_or_else(|_| "\"\"".into());
            format!(r#"{{"status":"updated","id":{id_json}}}"#)
        }
        QueryResult::Deleted { existed } => {
            format!(r#"{{"status":"deleted","existed":{existed}}}"#)
        }
        QueryResult::DeletedMany { count } => {
            format!(r#"{{"status":"deleted_many","count":{count}}}"#)
        }
        QueryResult::Batch {
            count,
            created,
            updated,
        } => {
            format!(
                r#"{{"status":"batch","count":{count},"created":{created},"updated":{updated}}}"#
            )
        }
        QueryResult::Rows(rows) => {
            let json_rows = serde_json::to_string(rows).unwrap_or_else(|_| "[]".into());
            format!(
                r#"{{"status":"rows","count":{},"data":{json_rows}}}"#,
                rows.len()
            )
        }
        QueryResult::Communities { algorithm, groups } => {
            let alg_json = serde_json::to_string(algorithm).unwrap_or_else(|_| "\"\"".into());
            let json_groups = serde_json::to_string(groups).unwrap_or_else(|_| "[]".into());
            format!(
                r#"{{"status":"communities","algorithm":{alg_json},"community_count":{},"data":{json_groups}}}"#,
                groups.len()
            )
        }
        QueryResult::Update { matched } => {
            format!(r#"{{"status":"update","matched":{matched}}}"#)
        }
        QueryResult::Count { count } => {
            format!(r#"{{"status":"count","count":{count}}}"#)
        }
        QueryResult::Components {
            count,
            largest,
            nodes,
        } => {
            format!(
                r#"{{"status":"components","component_count":{count},"largest":{largest},"nodes":{nodes}}}"#
            )
        }
        QueryResult::Compacted { reclaimed } => {
            format!(r#"{{"status":"compacted","reclaimed":{reclaimed}}}"#)
        }
    }
}

fn format_error_json(err: &dllb_core::Error) -> String {
    let msg_json = serde_json::to_string(&err.to_string()).unwrap_or_else(|_| "\"error\"".into());
    format!(r#"{{"status":"error","message":{msg_json}}}"#)
}

// ---------------------------------------------------------------------------
// TOON (TOML Object Notation)
// ---------------------------------------------------------------------------

fn format_result_toon(result: &QueryResult) -> String {
    match result {
        QueryResult::CachedResponse(s) => s.clone(),
        QueryResult::Ok => "status = \"ok\"".to_string(),
        QueryResult::Created { id } => {
            format!("status = \"created\"\nid = \"{id}\"")
        }
        QueryResult::Updated { id } => {
            format!("status = \"updated\"\nid = \"{id}\"")
        }
        QueryResult::Deleted { existed } => {
            format!("status = \"deleted\"\nexisted = {existed}")
        }
        QueryResult::DeletedMany { count } => {
            format!("status = \"deleted_many\"\ncount = {count}")
        }
        QueryResult::Batch {
            count,
            created,
            updated,
        } => {
            format!("status = \"batch\"\ncount = {count}\ncreated = {created}\nupdated = {updated}")
        }
        QueryResult::Rows(rows) => {
            let mut out = format!("status = \"rows\"\ncount = {}", rows.len());
            for row in rows {
                out.push_str("\n\n[[data]]");
                for (k, v) in row {
                    out.push_str(&format!("\n{k} = {}", toon_value(v)));
                }
            }
            out
        }
        QueryResult::Communities { algorithm, groups } => {
            let mut out = format!(
                "status = \"communities\"\nalgorithm = \"{algorithm}\"\ncommunity_count = {}",
                groups.len()
            );
            for row in groups {
                out.push_str("\n\n[[data]]");
                for (k, v) in row {
                    out.push_str(&format!("\n{k} = {}", toon_value(v)));
                }
            }
            out
        }
        QueryResult::Update { matched } => {
            format!("status = \"update\"\nmatched = {matched}")
        }
        QueryResult::Count { count } => {
            format!("status = \"count\"\ncount = {count}")
        }
        QueryResult::Components {
            count,
            largest,
            nodes,
        } => {
            format!(
                "status = \"components\"\ncomponent_count = {count}\nlargest = {largest}\nnodes = {nodes}"
            )
        }
        QueryResult::Compacted { reclaimed } => {
            format!("status = \"compacted\"\nreclaimed = {reclaimed}")
        }
    }
}

fn format_error_toon(err: &dllb_core::Error) -> String {
    let msg = err.to_string().replace('"', "\\\"");
    format!("status = \"error\"\nmessage = \"{msg}\"")
}

/// Encode a `Value` as a TOML literal.
fn toon_value(v: &Value) -> String {
    match v {
        Value::None => "\"none\"".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => format!("{f}"),
        Value::String(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        Value::Bytes(b) => format!("\"<{} bytes>\"", b.len()),
        Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(toon_value).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(map) => {
            let pairs: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{k} = {}", toon_value(v)))
                .collect();
            format!("{{ {} }}", pairs.join(", "))
        }
        Value::RecordId(rid) => format!("\"{rid}\""),
        Value::Vector(vec) => {
            let items: Vec<String> = vec.iter().map(|f| format!("{f}")).collect();
            format!("[{}]", items.join(", "))
        }
    }
}

// ---------------------------------------------------------------------------
// CSV
// ---------------------------------------------------------------------------

fn format_result_csv(result: &QueryResult) -> String {
    match result {
        QueryResult::CachedResponse(s) => s.clone(),
        QueryResult::Ok => "status\nok".to_string(),
        QueryResult::Created { id } => {
            format!("status,id\ncreated,{}", csv_escape(&id.to_string()))
        }
        QueryResult::Updated { id } => {
            format!("status,id\nupdated,{}", csv_escape(&id.to_string()))
        }
        QueryResult::Deleted { existed } => {
            format!("status,existed\ndeleted,{existed}")
        }
        QueryResult::DeletedMany { count } => {
            format!("status,count\ndeleted_many,{count}")
        }
        QueryResult::Batch {
            count,
            created,
            updated,
        } => {
            format!("status,count,created,updated\nbatch,{count},{created},{updated}")
        }
        QueryResult::Rows(rows) => format_rows_csv(rows),
        QueryResult::Communities { groups, .. } => format_rows_csv(groups),
        QueryResult::Update { matched } => format!("status,matched\nupdate,{matched}"),
        QueryResult::Count { count } => format!("status,count\ncount,{count}"),
        QueryResult::Components {
            count,
            largest,
            nodes,
        } => {
            format!("status,component_count,largest,nodes\ncomponents,{count},{largest},{nodes}")
        }
        QueryResult::Compacted { reclaimed } => {
            format!("status,reclaimed\ncompacted,{reclaimed}")
        }
    }
}

fn format_error_csv(err: &dllb_core::Error) -> String {
    format!("status,message\nerror,{}", csv_escape(&err.to_string()))
}

fn format_rows_csv(rows: &[BTreeMap<String, Value>]) -> String {
    if rows.is_empty() {
        return "status,count\nrows,0".into();
    }

    // Collect all column names (union of all rows) preserving BTreeMap order.
    let mut columns: Vec<String> = Vec::new();
    for row in rows {
        for key in row.keys() {
            if !columns.contains(key) {
                columns.push(key.clone());
            }
        }
    }

    let mut out = columns.join(",");
    for row in rows {
        out.push('\n');
        let cells: Vec<String> = columns
            .iter()
            .map(|col| match row.get(col) {
                Some(v) => csv_escape(&csv_value(v)),
                None => String::new(),
            })
            .collect();
        out.push_str(&cells.join(","));
    }
    out
}

fn csv_value(v: &Value) -> String {
    match v {
        Value::None => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => format!("{f}"),
        Value::String(s) => s.clone(),
        Value::Bytes(b) => format!("<{} bytes>", b.len()),
        Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(csv_value).collect();
            format!("[{}]", items.join(";"))
        }
        Value::Object(_) => "<object>".into(),
        Value::RecordId(rid) => rid.to_string(),
        Value::Vector(vec) => {
            let items: Vec<String> = vec.iter().map(|f| format!("{f}")).collect();
            format!("[{}]", items.join(";"))
        }
    }
}

/// Quote a CSV field if it contains commas, quotes, or newlines.
fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_error_json_escapes_backslashes_quotes_and_newlines() {
        let err = dllb_core::Error::Query(
            "unexpected character: '\\', near \"foo\", line1\nline2\t\x00".into(),
        );
        let json = format_error(&err, OutcomeFormat::Json);
        // Must be single-line valid JSON
        assert!(!json.contains('\n'));
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["status"], "error");
        assert_eq!(parsed["message"], err.to_string());
    }

    // -- Compacted ----------------------------------------------------------

    #[test]
    fn compacted_renders_in_every_format() {
        let yes = QueryResult::Compacted { reclaimed: true };
        assert_eq!(
            format_result(&yes, OutcomeFormat::Json),
            r#"{"status":"compacted","reclaimed":true}"#
        );
        assert_eq!(
            format_result(&yes, OutcomeFormat::Toon),
            "status = \"compacted\"\nreclaimed = true"
        );
        assert_eq!(
            format_result(&yes, OutcomeFormat::Csv),
            "status,reclaimed\ncompacted,true"
        );

        let no = QueryResult::Compacted { reclaimed: false };
        assert_eq!(
            format_result(&no, OutcomeFormat::Json),
            r#"{"status":"compacted","reclaimed":false}"#
        );
    }

    // -- JSON round-trip for ids containing quotes --------------------------

    #[test]
    fn created_id_with_quote_stays_valid_json() {
        let result = QueryResult::Created {
            id: dllb_core::RecordId::new("user", "o'brien"),
        };
        let json = format_result(&result, OutcomeFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["id"], "user:o'brien");
    }

    // -- TOON ---------------------------------------------------------------

    #[test]
    fn toon_escapes_quotes_and_backslashes_in_strings() {
        assert_eq!(
            toon_value(&Value::String(r#"a"b\c"#.into())),
            r#""a\"b\\c""#
        );
    }

    #[test]
    fn toon_rows_emit_one_table_per_row() {
        let mut row = BTreeMap::new();
        row.insert("name".to_string(), Value::String("Alice".into()));
        let out = format_result(&QueryResult::Rows(vec![row]), OutcomeFormat::Toon);
        assert!(out.starts_with("status = \"rows\"\ncount = 1"));
        assert!(out.contains("[[data]]"));
        assert!(out.contains("name = \"Alice\""));
    }

    // -- CSV ----------------------------------------------------------------

    #[test]
    fn csv_escapes_commas_quotes_and_newlines() {
        assert_eq!(csv_escape("plain"), "plain");
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("a\"b"), "\"a\"\"b\"");
        assert_eq!(csv_escape("a\nb"), "\"a\nb\"");
    }

    #[test]
    fn csv_rows_union_columns_across_heterogeneous_rows() {
        let mut first = BTreeMap::new();
        first.insert("a".to_string(), Value::Int(1));
        let mut second = BTreeMap::new();
        second.insert("b".to_string(), Value::Int(2));

        let out = format_result(&QueryResult::Rows(vec![first, second]), OutcomeFormat::Csv);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "a,b");
        // Missing cells stay empty rather than shifting columns.
        assert_eq!(lines[1], "1,");
        assert_eq!(lines[2], ",2");
    }

    #[test]
    fn csv_empty_rows_reports_zero_count() {
        let out = format_result(&QueryResult::Rows(vec![]), OutcomeFormat::Csv);
        assert_eq!(out, "status,count\nrows,0");
    }

    #[test]
    fn csv_object_value_is_rendered_as_a_placeholder() {
        // A nested object has no flat CSV representation; the placeholder must
        // be stable so downstream parsers can recognise it.
        assert_eq!(csv_value(&Value::Object(BTreeMap::new())), "<object>");
    }
}
