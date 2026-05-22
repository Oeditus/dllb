//! JSON formatting for [`QueryResult`] values.
//!
//! Produces JSON strings suitable for wire protocol responses and
//! CLI output. Reused by both `dllb-server` and `dllb-cli`.

use crate::executor::QueryResult;

/// Format a successful `QueryResult` as a JSON string.
pub fn format_result(result: &QueryResult) -> String {
    match result {
        QueryResult::Ok => r#"{"status":"ok"}"#.to_string(),
        QueryResult::Created { id } => {
            format!(r#"{{"status":"created","id":"{}"}}"#, id)
        }
        QueryResult::Deleted { existed } => {
            format!(r#"{{"status":"deleted","existed":{existed}}}"#)
        }
        QueryResult::Rows(rows) => {
            let json_rows = serde_json::to_string(rows).unwrap_or_else(|_| "[]".into());
            format!(
                r#"{{"status":"rows","count":{},"data":{json_rows}}}"#,
                rows.len()
            )
        }
    }
}

/// Format an error as a JSON string.
pub fn format_error(err: &dllb_core::Error) -> String {
    let msg = err.to_string().replace('"', "\\\"");
    format!(r#"{{"status":"error","message":"{msg}"}}"#)
}
