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
// Plain JSON projection
// ---------------------------------------------------------------------------
//
// `Value`'s `Serialize` impl is the MessagePack *storage* encoding: it tags
// every variant (`{"String":"Alice"}`), which is correct for round-tripping
// through redb but hostile as a client-facing wire format. Rows are therefore
// projected into plain `serde_json::Value` before serialisation, so clients
// see `{"name":"Alice"}`. The storage encoding is untouched.

/// Project a [`Value`] into plain JSON.
///
/// Lossy only where JSON has no equivalent: `Bytes` becomes a byte-count
/// placeholder and `RecordId` becomes its `table:id` string.
fn to_plain_json(value: &Value) -> serde_json::Value {
    use serde_json::Value as J;
    match value {
        Value::None => J::Null,
        Value::Bool(b) => J::Bool(*b),
        Value::Int(n) => J::Number((*n).into()),
        Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(J::Number)
            // NaN/Infinity have no JSON representation; render as null
            // rather than emitting invalid JSON.
            .unwrap_or(J::Null),
        Value::String(s) => J::String(s.clone()),
        Value::Bytes(b) => J::String(format!("<{} bytes>", b.len())),
        Value::RecordId(id) => J::String(id.to_string()),
        Value::Array(items) => J::Array(items.iter().map(to_plain_json).collect()),
        Value::Object(map) => J::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), to_plain_json(v)))
                .collect(),
        ),
        Value::Vector(v) => J::Array(
            v.iter()
                .map(|f| {
                    serde_json::Number::from_f64(*f as f64)
                        .map(J::Number)
                        .unwrap_or(J::Null)
                })
                .collect(),
        ),
    }
}

/// Project a result row into a plain JSON object.
fn row_to_plain_json(row: &BTreeMap<String, Value>) -> serde_json::Value {
    serde_json::Value::Object(
        row.iter()
            .map(|(k, v)| (k.clone(), to_plain_json(v)))
            .collect(),
    )
}

/// Serialise rows as a plain JSON array, falling back to `[]` on failure.
fn rows_to_plain_json(rows: &[BTreeMap<String, Value>]) -> String {
    let projected: Vec<serde_json::Value> = rows.iter().map(row_to_plain_json).collect();
    serde_json::to_string(&projected).unwrap_or_else(|_| "[]".into())
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
            let json_rows = rows_to_plain_json(rows);
            format!(
                r#"{{"status":"rows","count":{},"data":{json_rows}}}"#,
                rows.len()
            )
        }
        QueryResult::Communities { algorithm, groups } => {
            let alg_json = serde_json::to_string(algorithm).unwrap_or_else(|_| "\"\"".into());
            let json_groups = rows_to_plain_json(groups);
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

    // -- plain JSON projection ----------------------------------------------

    #[test]
    fn json_rows_are_plain_not_serde_tagged() {
        // The wire format must not leak `Value`'s MessagePack variant tags.
        let mut row = BTreeMap::new();
        row.insert("name".to_string(), Value::String("Alice".into()));
        row.insert("age".to_string(), Value::Int(30));

        let out = format_result(&QueryResult::Rows(vec![row]), OutcomeFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(parsed["data"][0]["name"], "Alice");
        assert_eq!(parsed["data"][0]["age"], 30);
        // No variant tagging anywhere in the payload.
        assert!(!out.contains("\"String\""), "leaked serde tag: {out}");
        assert!(!out.contains("\"Int\""), "leaked serde tag: {out}");
    }

    #[test]
    fn json_projection_covers_every_value_variant() {
        let mut obj = BTreeMap::new();
        obj.insert("k".to_string(), Value::Bool(true));

        let mut row = BTreeMap::new();
        row.insert("none".to_string(), Value::None);
        row.insert("bool".to_string(), Value::Bool(false));
        row.insert("int".to_string(), Value::Int(-1));
        row.insert("float".to_string(), Value::Float(1.5));
        row.insert("string".to_string(), Value::String("s".into()));
        row.insert("bytes".to_string(), Value::Bytes(vec![0; 4]));
        row.insert(
            "array".to_string(),
            Value::Array(vec![Value::Int(1), Value::String("a".into())]),
        );
        row.insert("object".to_string(), Value::Object(obj));
        row.insert(
            "record".to_string(),
            Value::RecordId(dllb_core::RecordId::new("user", "alice")),
        );
        row.insert("vector".to_string(), Value::Vector(vec![0.5, 1.0]));

        let out = format_result(&QueryResult::Rows(vec![row]), OutcomeFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        let r = &parsed["data"][0];

        assert_eq!(r["none"], serde_json::Value::Null);
        assert_eq!(r["bool"], false);
        assert_eq!(r["int"], -1);
        assert_eq!(r["float"], 1.5);
        assert_eq!(r["string"], "s");
        assert_eq!(r["bytes"], "<4 bytes>");
        assert_eq!(r["array"], serde_json::json!([1, "a"]));
        assert_eq!(r["object"], serde_json::json!({"k": true}));
        assert_eq!(r["record"], "user:alice");
        assert_eq!(r["vector"], serde_json::json!([0.5, 1.0]));
    }

    #[test]
    fn json_projection_renders_non_finite_floats_as_null() {
        // NaN/Infinity are not representable in JSON; emitting them would
        // produce a body no client can parse.
        let mut row = BTreeMap::new();
        row.insert("nan".to_string(), Value::Float(f64::NAN));
        row.insert("inf".to_string(), Value::Float(f64::INFINITY));

        let out = format_result(&QueryResult::Rows(vec![row]), OutcomeFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(parsed["data"][0]["nan"], serde_json::Value::Null);
        assert_eq!(parsed["data"][0]["inf"], serde_json::Value::Null);
    }

    #[test]
    fn json_communities_rows_are_plain_too() {
        let mut group = BTreeMap::new();
        group.insert("id".to_string(), Value::String("c1".into()));
        group.insert(
            "members".to_string(),
            Value::Array(vec![Value::String("a".into())]),
        );

        let out = format_result(
            &QueryResult::Communities {
                algorithm: "louvain".into(),
                groups: vec![group],
            },
            OutcomeFormat::Json,
        );
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(parsed["data"][0]["id"], "c1");
        assert_eq!(parsed["data"][0]["members"], serde_json::json!(["a"]));
    }

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
