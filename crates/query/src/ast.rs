//! Abstract Syntax Tree types for the dllb query language.

/// Requested output format for a query result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum OutcomeFormat {
    /// JSON (the default wire format).
    #[default]
    Json,
    /// TOON -- TOML Object Notation.
    Toon,
    /// Comma-separated values.
    Csv,
}

/// A parsed query: statement plus optional output format.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub statement: Statement,
    pub outcome: OutcomeFormat,
}

/// Behaviour when a `CREATE` hits an existing record.
#[derive(Debug, Clone, PartialEq)]
pub enum OnConflict {
    /// `ON CONFLICT UPDATE` -- merge the CREATE fields into the existing record.
    Update,
    /// `ON CONFLICT UPDATE SET field = value, ...` -- apply explicit fields to
    /// the existing record instead of the CREATE fields.
    UpdateSet(Vec<(String, Literal)>),
}

/// Community detection algorithm named in a `GRAPH COMMUNITIES` statement.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CommunityAlgorithm {
    /// Louvain modularity optimisation (default).
    #[default]
    Louvain,
    /// Label propagation.
    LabelPropagation,
}

/// A parsed query statement.
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    /// `CREATE table SET field = value, ... [ON CONFLICT UPDATE [SET ...]];`
    Create {
        table: String,
        id: Option<String>,
        fields: Vec<(String, Literal)>,
        on_conflict: Option<OnConflict>,
    },
    /// `SELECT fields FROM target [WHERE clause] [LIMIT n];`
    Select {
        fields: SelectFields,
        from: FromTarget,
        filter: Option<WhereClause>,
        limit: Option<u64>,
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
    /// `GRAPH COMMUNITIES <table> [ALGORITHM louvain|lp] [MAX_ITER n] [RESOLUTION f]`
    GraphCommunities {
        /// Edge table (edge type) to compute communities over.
        table: String,
        algorithm: CommunityAlgorithm,
        max_iterations: usize,
        resolution: f64,
    },
    /// `UPDATE <table>[:<id>] SET field = value, ... [WHERE clause]`
    ///
    /// `SET` applies partial-update (merge) semantics: only the listed fields
    /// are changed. When `target` is a `Record`, `filter` is `None` and at most
    /// one row is affected; when `target` is a `Table`, the optional `filter`
    /// selects the rows to update.
    Update {
        target: FromTarget,
        fields: Vec<(String, Literal)>,
        filter: Option<WhereClause>,
    },
    /// `COUNT <table> [WHERE clause]` -- server-side row count.
    Count {
        table: String,
        filter: Option<WhereClause>,
    },
    /// `GRAPH COMPONENTS <table>` -- connected components over an edge table.
    GraphComponents {
        /// Edge table (edge type) to compute connected components over.
        table: String,
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
    /// `field IS [NOT] NONE` -- null / not-null predicate. `negated` is `true`
    /// for `IS NOT NONE`.
    IsNull { field: String, negated: bool },
}

/// A literal value in the query language.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    None,
    /// `[literal, literal, ...]` -- an array literal (e.g. a dense embedding).
    Array(Vec<Literal>),
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
            Literal::Array(items) => {
                dllb_core::Value::Array(items.iter().map(Literal::to_value).collect())
            }
        }
    }
}
