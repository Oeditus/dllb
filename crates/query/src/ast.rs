//! Abstract Syntax Tree types for the dllb query language.

/// A parsed query statement.
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    /// `CREATE table SET field = value, ...;`
    Create {
        table: String,
        id: Option<String>,
        fields: Vec<(String, Literal)>,
    },
    /// `SELECT fields FROM target [WHERE clause];`
    Select {
        fields: SelectFields,
        from: FromTarget,
        filter: Option<WhereClause>,
    },
    /// `DELETE table:id;`
    Delete { table: String, id: String },
    /// `RELATE src->edge_type->dst [SET field = value, ...];`
    Relate {
        src: RecordRef,
        edge_type: String,
        dst: RecordRef,
        fields: Vec<(String, Literal)>,
    },
}

/// Which fields to return in a SELECT.
#[derive(Debug, Clone, PartialEq)]
pub enum SelectFields {
    /// `SELECT *`
    All,
    /// `SELECT name, age`
    Named(Vec<String>),
}

/// A reference to a specific record: `table:id`.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordRef {
    pub table: String,
    pub id: String,
}

/// The FROM target of a SELECT.
#[derive(Debug, Clone, PartialEq)]
pub enum FromTarget {
    /// `FROM table` -- scan all records.
    Table(String),
    /// `FROM table:id` -- point lookup.
    Record(RecordRef),
}

/// A WHERE filter clause (only equality for now).
#[derive(Debug, Clone, PartialEq)]
pub enum WhereClause {
    /// `WHERE field = value`
    Eq { field: String, value: Literal },
}

/// A literal value in the query language.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    None,
}

impl Literal {
    /// Convert to a `dllb_core::Value`.
    pub fn to_value(&self) -> dllb_core::Value {
        match self {
            Literal::String(s) => dllb_core::Value::String(s.clone()),
            Literal::Int(n) => dllb_core::Value::Int(*n),
            Literal::Float(f) => dllb_core::Value::Float(*f),
            Literal::Bool(b) => dllb_core::Value::Bool(*b),
            Literal::None => dllb_core::Value::None,
        }
    }
}
