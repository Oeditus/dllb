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
    /// `SELECT ->edge->table[.field]` -- graph traversal projection.
    Traversal(TraversalChain),
}

/// A graph traversal chain used as SELECT fields.
///
/// Examples:
/// - `->knows->user`            -- follow outgoing "knows" edges, return user records
/// - `->knows->user.name`       -- same, project only the "name" field
/// - `<-likes<-user`            -- follow incoming "likes" edges
/// - `->knows->user->likes->product.name` -- two-hop chain
#[derive(Debug, Clone, PartialEq)]
pub struct TraversalChain {
    /// Ordered sequence of hops to follow.
    pub hops: Vec<TraversalHop>,
    /// If `Some(field)`, project only that field from the destination records.
    /// If `None`, return the full destination record.
    pub projection: Option<String>,
}

/// A single hop in a traversal chain.
#[derive(Debug, Clone, PartialEq)]
pub struct TraversalHop {
    pub direction: TraversalDirection,
    /// Edge type / relation name (the table used by `EdgeStore`).
    pub edge_type: String,
    /// Table name of the destination records (used to look them up via `Collection`).
    pub dest_table: String,
}

/// Direction of a graph traversal hop.
#[derive(Debug, Clone, PartialEq)]
pub enum TraversalDirection {
    /// Outgoing edges (`->edge_type->dest`).
    Out,
    /// Incoming edges (`<-edge_type<-dest`).
    In,
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

/// Comparison operator used in WHERE conditions.
#[derive(Debug, Clone, PartialEq)]
pub enum CmpOp {
    Eq,  // =
    Ne,  // !=
    Gt,  // >
    Lt,  // <
    Gte, // >=
    Lte, // <=
}

/// A WHERE filter expression.
///
/// Supports arbitrary nesting via `And`. `Or` is left for a future phase.
#[derive(Debug, Clone, PartialEq)]
pub enum WhereClause {
    /// `field op value`  (e.g. `age >= 30`, `name != 'Bob'`)
    Cmp {
        field: String,
        op: CmpOp,
        value: Literal,
    },
    /// `left AND right`
    And(Box<WhereClause>, Box<WhereClause>),
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
