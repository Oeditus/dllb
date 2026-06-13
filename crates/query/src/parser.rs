//! Recursive-descent parser for the dllb query language.
//!
//! Parses a token stream into an [`Statement`] AST node.

use dllb_core::{Error, Result};

use crate::ast::*;
use crate::tokenizer::{Token, tokenize};

/// Parse an input string into a [`Query`].
pub fn parse(input: &str) -> Result<Query> {
    let tokens = tokenize(input)?;
    let mut pos = 0;
    let statement = parse_statement(&tokens, &mut pos)?;
    let outcome = parse_outcome(&tokens, &mut pos)?;
    // Optional trailing semicolon.
    if pos < tokens.len() && tokens[pos] == Token::Semicolon {
        pos += 1;
    }
    if pos < tokens.len() {
        return Err(Error::Query(format!(
            "unexpected token after statement: {:?}",
            tokens[pos]
        )));
    }
    Ok(Query { statement, outcome })
}

fn parse_statement(tokens: &[Token], pos: &mut usize) -> Result<Statement> {
    match tokens.get(*pos) {
        Some(Token::Create) => parse_create(tokens, pos),
        Some(Token::Select) => parse_select(tokens, pos),
        Some(Token::Delete) => parse_delete(tokens, pos),
        Some(Token::Relate) => parse_relate(tokens, pos),
        Some(Token::Update) => parse_update(tokens, pos),
        Some(Token::Count) => parse_count(tokens, pos),
        Some(Token::Graph) => parse_graph(tokens, pos),
        // DDL/search keywords are contextual idents (not reserved tokens) so
        // they never shadow table/field names.
        Some(Token::Ident(kw)) if kw.eq_ignore_ascii_case("DEFINE") => parse_define(tokens, pos),
        Some(Token::Ident(kw)) if kw.eq_ignore_ascii_case("REMOVE") => {
            parse_remove_index(tokens, pos)
        }
        Some(Token::Ident(kw)) if kw.eq_ignore_ascii_case("SEARCH") => parse_search(tokens, pos),
        Some(Token::Ident(kw)) if kw.eq_ignore_ascii_case("VECTOR") => {
            parse_vector_search(tokens, pos)
        }
        Some(Token::Ident(kw)) if kw.eq_ignore_ascii_case("HYBRID") => parse_hybrid(tokens, pos),
        Some(t) => Err(Error::Query(format!("expected statement, got {t:?}"))),
        None => Err(Error::Query("empty input".into())),
    }
}

// -----------------------------------------------------------------------
// DEFINE INDEX <name> ON [TABLE] <table> FIELDS <field>[, ...] [UNIQUE]
// REMOVE INDEX <name> ON [TABLE] <table>
// -----------------------------------------------------------------------

fn parse_define(tokens: &[Token], pos: &mut usize) -> Result<Statement> {
    expect_keyword(tokens, pos, "DEFINE")?;
    if consume_keyword(tokens, pos, "FULLTEXT") {
        expect_keyword(tokens, pos, "INDEX")?;
        return parse_define_fulltext(tokens, pos);
    }
    if consume_keyword(tokens, pos, "VECTOR") {
        expect_keyword(tokens, pos, "INDEX")?;
        return parse_define_vector(tokens, pos);
    }
    expect_keyword(tokens, pos, "INDEX")?;
    parse_define_btree(tokens, pos)
}

fn parse_define_btree(tokens: &[Token], pos: &mut usize) -> Result<Statement> {
    let name = expect_ident(tokens, pos)?;
    let table = parse_on_table(tokens, pos)?;
    expect_keyword(tokens, pos, "FIELDS")?;

    let mut fields = vec![expect_ident(tokens, pos)?];
    while matches!(tokens.get(*pos), Some(Token::Comma)) {
        *pos += 1;
        fields.push(expect_ident(tokens, pos)?);
    }

    let unique = consume_keyword(tokens, pos, "UNIQUE");

    Ok(Statement::DefineIndex {
        name,
        table,
        fields,
        unique,
    })
}

fn parse_define_fulltext(tokens: &[Token], pos: &mut usize) -> Result<Statement> {
    let name = expect_ident(tokens, pos)?;
    let table = parse_on_table(tokens, pos)?;
    expect_keyword(tokens, pos, "FIELDS")?;
    let field = expect_ident(tokens, pos)?;
    let analyzer = if consume_keyword(tokens, pos, "ANALYZER") {
        Some(expect_ident(tokens, pos)?)
    } else {
        None
    };
    Ok(Statement::DefineFulltextIndex {
        name,
        table,
        field,
        analyzer,
    })
}

fn parse_define_vector(tokens: &[Token], pos: &mut usize) -> Result<Statement> {
    let name = expect_ident(tokens, pos)?;
    let table = parse_on_table(tokens, pos)?;
    expect_keyword(tokens, pos, "FIELDS")?;
    let field = expect_ident(tokens, pos)?;
    expect_keyword(tokens, pos, "DIMENSION")?;
    let dim = expect_positive_int(tokens, pos, "DIMENSION")? as usize;
    let metric = if consume_keyword(tokens, pos, "METRIC") {
        Some(expect_ident(tokens, pos)?)
    } else {
        None
    };
    Ok(Statement::DefineVectorIndex {
        name,
        table,
        field,
        dim,
        metric,
    })
}

// -----------------------------------------------------------------------
// SEARCH <table> <field> '<query>' [WHERE clause] [LIMIT n]
// VECTOR SEARCH <table> <field> [v1, v2, ...] [WHERE clause] [K n]
// HYBRID SEARCH <table> TEXT <field> '<q>' VECTOR <field> [..]
//        [ALPHA f] [WHERE clause] [LIMIT n]
// -----------------------------------------------------------------------

fn parse_search(tokens: &[Token], pos: &mut usize) -> Result<Statement> {
    expect_keyword(tokens, pos, "SEARCH")?;
    let table = expect_ident(tokens, pos)?;
    let field = expect_ident(tokens, pos)?;
    let query = expect_string(tokens, pos)?;
    let filter = parse_where_clause(tokens, pos)?;
    let limit = parse_limit(tokens, pos)?;
    Ok(Statement::Search {
        table,
        field,
        query,
        filter,
        limit,
    })
}

fn parse_vector_search(tokens: &[Token], pos: &mut usize) -> Result<Statement> {
    expect_keyword(tokens, pos, "VECTOR")?;
    expect_keyword(tokens, pos, "SEARCH")?;
    let table = expect_ident(tokens, pos)?;
    let field = expect_ident(tokens, pos)?;
    let vector = parse_vector_literal(tokens, pos)?;
    let filter = parse_where_clause(tokens, pos)?;
    let k = if consume_keyword(tokens, pos, "K") {
        Some(expect_positive_int(tokens, pos, "K")?)
    } else {
        None
    };
    Ok(Statement::VectorSearch {
        table,
        field,
        vector,
        filter,
        k,
    })
}

fn parse_hybrid(tokens: &[Token], pos: &mut usize) -> Result<Statement> {
    expect_keyword(tokens, pos, "HYBRID")?;
    expect_keyword(tokens, pos, "SEARCH")?;
    let table = expect_ident(tokens, pos)?;
    expect_keyword(tokens, pos, "TEXT")?;
    let text_field = expect_ident(tokens, pos)?;
    let query = expect_string(tokens, pos)?;
    expect_keyword(tokens, pos, "VECTOR")?;
    let vector_field = expect_ident(tokens, pos)?;
    let vector = parse_vector_literal(tokens, pos)?;
    let alpha = if consume_keyword(tokens, pos, "ALPHA") {
        Some(expect_number(tokens, pos, "ALPHA")?)
    } else {
        None
    };
    let filter = parse_where_clause(tokens, pos)?;
    let limit = parse_limit(tokens, pos)?;
    Ok(Statement::HybridSearch {
        table,
        text_field,
        query,
        vector_field,
        vector,
        alpha,
        filter,
        limit,
    })
}

/// Parse a numeric array literal `[..]` into `Vec<f64>` (ints coerce to f64).
fn parse_vector_literal(tokens: &[Token], pos: &mut usize) -> Result<Vec<f64>> {
    match parse_literal(tokens, pos)? {
        Literal::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for it in items {
                match it {
                    Literal::Float(f) => out.push(f),
                    Literal::Int(n) => out.push(n as f64),
                    other => {
                        return Err(Error::Query(format!(
                            "vector elements must be numbers, got {other:?}"
                        )));
                    }
                }
            }
            Ok(out)
        }
        other => Err(Error::Query(format!(
            "expected a vector literal [..], got {other:?}"
        ))),
    }
}

/// Parse `ON [TABLE] <table>` and return the table name.
fn parse_on_table(tokens: &[Token], pos: &mut usize) -> Result<String> {
    expect(tokens, pos, &Token::On)?;
    consume_keyword(tokens, pos, "TABLE"); // optional `ON TABLE`
    expect_ident(tokens, pos)
}

fn parse_remove_index(tokens: &[Token], pos: &mut usize) -> Result<Statement> {
    expect_keyword(tokens, pos, "REMOVE")?;
    expect_keyword(tokens, pos, "INDEX")?;
    let name = expect_ident(tokens, pos)?;
    expect(tokens, pos, &Token::On)?;
    consume_keyword(tokens, pos, "TABLE"); // optional `ON TABLE`
    let table = expect_ident(tokens, pos)?;
    Ok(Statement::RemoveIndex { name, table })
}

// -----------------------------------------------------------------------
// CREATE table [:id] SET field = value, ...
// -----------------------------------------------------------------------

fn parse_create(tokens: &[Token], pos: &mut usize) -> Result<Statement> {
    expect(tokens, pos, &Token::Create)?;
    let table = expect_ident(tokens, pos)?;

    // Optional :id
    let id = if matches!(tokens.get(*pos), Some(Token::Colon)) {
        *pos += 1;
        Some(expect_ident(tokens, pos)?)
    } else {
        None
    };

    expect(tokens, pos, &Token::Set)?;
    let fields = parse_set_clause(tokens, pos)?;

    let on_conflict = parse_on_conflict(tokens, pos)?;

    Ok(Statement::Create {
        table,
        id,
        fields,
        on_conflict,
    })
}

// -----------------------------------------------------------------------
// SELECT fields FROM target [WHERE clause]
// -----------------------------------------------------------------------

fn parse_select(tokens: &[Token], pos: &mut usize) -> Result<Statement> {
    expect(tokens, pos, &Token::Select)?;

    let fields = if matches!(tokens.get(*pos), Some(Token::Star)) {
        *pos += 1;
        SelectFields::All
    } else if matches!(
        tokens.get(*pos),
        Some(Token::Arrow) | Some(Token::BackArrow)
    ) {
        // Graph traversal: ->edge->table[.field]  or  <-edge<-table[.field]
        SelectFields::Traversal(parse_traversal_chain(tokens, pos)?)
    } else {
        let mut names = vec![expect_ident(tokens, pos)?];
        while matches!(tokens.get(*pos), Some(Token::Comma)) {
            *pos += 1;
            names.push(expect_ident(tokens, pos)?);
        }
        SelectFields::Named(names)
    };

    expect(tokens, pos, &Token::From)?;

    let table = expect_ident(tokens, pos)?;
    let from = if matches!(tokens.get(*pos), Some(Token::Colon)) {
        *pos += 1;
        let id = expect_ident(tokens, pos)?;
        FromTarget::Record(RecordRef { table, id })
    } else {
        FromTarget::Table(table)
    };

    let filter = parse_where_clause(tokens, pos)?;
    let order_by = parse_order_by(tokens, pos)?;
    let limit = parse_limit(tokens, pos)?;

    Ok(Statement::Select {
        fields,
        from,
        filter,
        order_by,
        limit,
    })
}

// -----------------------------------------------------------------------
// LIMIT n
// -----------------------------------------------------------------------

fn parse_limit(tokens: &[Token], pos: &mut usize) -> Result<Option<u64>> {
    if !matches!(tokens.get(*pos), Some(Token::Limit)) {
        return Ok(None);
    }
    *pos += 1;
    match tokens.get(*pos) {
        Some(Token::IntLit(n)) if *n > 0 => {
            let v = *n as u64;
            *pos += 1;
            Ok(Some(v))
        }
        Some(Token::IntLit(n)) => Err(Error::Query(format!(
            "LIMIT must be a positive integer, got {n}"
        ))),
        Some(t) => Err(Error::Query(format!(
            "expected positive integer after LIMIT, got {t:?}"
        ))),
        None => Err(Error::Query("unexpected end of input after LIMIT".into())),
    }
}

// -----------------------------------------------------------------------
// Traversal chain:  ->edge->dest[->edge->dest...][.field]
//                  <-edge<-dest[<-edge<-dest...][.field]
// -----------------------------------------------------------------------

fn parse_traversal_chain(tokens: &[Token], pos: &mut usize) -> Result<TraversalChain> {
    let mut hops = Vec::new();

    loop {
        // Each hop starts with an Arrow or BackArrow.
        let dir = match tokens.get(*pos) {
            Some(Token::Arrow) => {
                *pos += 1;
                TraversalDirection::Out
            }
            Some(Token::BackArrow) => {
                *pos += 1;
                TraversalDirection::In
            }
            _ => break,
        };

        let edge_type = expect_ident(tokens, pos)?;

        // Closing arrow must match the opening direction.
        match (&dir, tokens.get(*pos)) {
            (TraversalDirection::Out, Some(Token::Arrow)) => *pos += 1,
            (TraversalDirection::In, Some(Token::BackArrow)) => *pos += 1,
            (_, Some(t)) => {
                return Err(Error::Query(format!(
                    "traversal: expected matching arrow after edge type, got {t:?}"
                )));
            }
            (_, None) => {
                return Err(Error::Query(
                    "traversal: unexpected end after edge type".into(),
                ));
            }
        }

        let dest_table = expect_ident(tokens, pos)?;
        hops.push(TraversalHop {
            direction: dir,
            edge_type,
            dest_table,
        });

        // Continue only if the next token starts another hop.
        if !matches!(
            tokens.get(*pos),
            Some(Token::Arrow) | Some(Token::BackArrow)
        ) {
            break;
        }
    }

    if hops.is_empty() {
        return Err(Error::Query("empty traversal chain".into()));
    }

    // Optional field projection: .field_name
    let projection = if matches!(tokens.get(*pos), Some(Token::Dot)) {
        *pos += 1;
        Some(expect_ident(tokens, pos)?)
    } else {
        None
    };

    Ok(TraversalChain { hops, projection })
}

// -----------------------------------------------------------------------
// DELETE table:id
// -----------------------------------------------------------------------

fn parse_delete(tokens: &[Token], pos: &mut usize) -> Result<Statement> {
    expect(tokens, pos, &Token::Delete)?;
    let table = expect_ident(tokens, pos)?;
    // `DELETE table:id` is a point delete; `DELETE table [WHERE ...]` deletes
    // every matching row.
    if matches!(tokens.get(*pos), Some(Token::Colon)) {
        *pos += 1;
        let id = expect_ident(tokens, pos)?;
        Ok(Statement::Delete { table, id })
    } else {
        let filter = parse_where_clause(tokens, pos)?;
        Ok(Statement::DeleteWhere { table, filter })
    }
}

// -----------------------------------------------------------------------
// UPDATE table[:id] SET field = value, ... [WHERE clause]
// -----------------------------------------------------------------------

fn parse_update(tokens: &[Token], pos: &mut usize) -> Result<Statement> {
    expect(tokens, pos, &Token::Update)?;
    let table = expect_ident(tokens, pos)?;

    // Optional :id selects a single record; otherwise the whole table.
    let target = if matches!(tokens.get(*pos), Some(Token::Colon)) {
        *pos += 1;
        let id = expect_ident(tokens, pos)?;
        FromTarget::Record(RecordRef { table, id })
    } else {
        FromTarget::Table(table)
    };

    expect(tokens, pos, &Token::Set)?;
    let fields = parse_set_clause(tokens, pos)?;
    let filter = parse_where_clause(tokens, pos)?;

    Ok(Statement::Update {
        target,
        fields,
        filter,
    })
}

// -----------------------------------------------------------------------
// COUNT table [WHERE clause]
// -----------------------------------------------------------------------

fn parse_count(tokens: &[Token], pos: &mut usize) -> Result<Statement> {
    expect(tokens, pos, &Token::Count)?;
    let table = expect_ident(tokens, pos)?;
    let filter = parse_where_clause(tokens, pos)?;
    let group_by = parse_group_by(tokens, pos)?;
    Ok(Statement::Count {
        table,
        filter,
        group_by,
    })
}

/// Parse an optional `GROUP BY <field>` clause.
fn parse_group_by(tokens: &[Token], pos: &mut usize) -> Result<Option<String>> {
    if !consume_keyword(tokens, pos, "GROUP") {
        return Ok(None);
    }
    expect_keyword(tokens, pos, "BY")?;
    Ok(Some(expect_ident(tokens, pos)?))
}

// -----------------------------------------------------------------------
// GRAPH COMMUNITIES <table> [ALGORITHM louvain|lp] [MAX_ITER n] [RESOLUTION f]
// -----------------------------------------------------------------------

fn parse_graph(tokens: &[Token], pos: &mut usize) -> Result<Statement> {
    expect(tokens, pos, &Token::Graph)?;
    match tokens.get(*pos) {
        Some(Token::Communities) => parse_graph_communities(tokens, pos),
        Some(Token::Components) => parse_graph_components(tokens, pos),
        // Analytics verbs are contextual idents, not reserved tokens.
        Some(Token::Ident(kw)) if kw.eq_ignore_ascii_case("PAGERANK") => {
            parse_graph_pagerank(tokens, pos)
        }
        Some(Token::Ident(kw)) if kw.eq_ignore_ascii_case("CENTRALITY") => {
            parse_graph_centrality(tokens, pos)
        }
        Some(Token::Ident(kw)) if kw.eq_ignore_ascii_case("PATH") => parse_graph_path(tokens, pos),
        Some(Token::Ident(kw)) if kw.eq_ignore_ascii_case("EDGES") => {
            parse_graph_edges(tokens, pos)
        }
        Some(t) => Err(Error::Query(format!(
            "expected COMMUNITIES, COMPONENTS, PAGERANK, CENTRALITY, PATH, or EDGES after GRAPH, got {t:?}"
        ))),
        None => Err(Error::Query("unexpected end of input after GRAPH".into())),
    }
}

// -----------------------------------------------------------------------
// GRAPH PAGERANK <table> [DAMPING f] [MAX_ITER n] [LIMIT n]
// GRAPH CENTRALITY <table> [DEGREE|INDEGREE|OUTDEGREE] [LIMIT n]
// GRAPH PATH <src> -> <dst> ON <table> [MAX_DEPTH n]
// GRAPH EDGES <table> [WHERE clause]
// -----------------------------------------------------------------------

fn parse_graph_pagerank(tokens: &[Token], pos: &mut usize) -> Result<Statement> {
    expect_keyword(tokens, pos, "PAGERANK")?;
    let table = expect_ident(tokens, pos)?;
    let mut damping = 0.85;
    let mut max_iterations = 100usize;
    loop {
        if consume_keyword(tokens, pos, "DAMPING") {
            damping = expect_number(tokens, pos, "DAMPING")?;
        } else if consume_keyword(tokens, pos, "MAX_ITER") {
            max_iterations = expect_positive_int(tokens, pos, "MAX_ITER")? as usize;
        } else {
            break;
        }
    }
    let limit = parse_limit(tokens, pos)?;
    Ok(Statement::GraphPagerank {
        table,
        damping,
        max_iterations,
        limit,
    })
}

fn parse_graph_centrality(tokens: &[Token], pos: &mut usize) -> Result<Statement> {
    expect_keyword(tokens, pos, "CENTRALITY")?;
    let table = expect_ident(tokens, pos)?;
    let mode = if consume_keyword(tokens, pos, "INDEGREE") {
        CentralityMode::InDegree
    } else if consume_keyword(tokens, pos, "OUTDEGREE") {
        CentralityMode::OutDegree
    } else {
        consume_keyword(tokens, pos, "DEGREE"); // optional explicit default
        CentralityMode::Degree
    };
    let limit = parse_limit(tokens, pos)?;
    Ok(Statement::GraphCentrality { table, mode, limit })
}

fn parse_graph_path(tokens: &[Token], pos: &mut usize) -> Result<Statement> {
    expect_keyword(tokens, pos, "PATH")?;
    let src = expect_ident(tokens, pos)?;
    expect(tokens, pos, &Token::Arrow)?;
    let dst = expect_ident(tokens, pos)?;
    expect(tokens, pos, &Token::On)?;
    let table = expect_ident(tokens, pos)?;
    let max_depth = if consume_keyword(tokens, pos, "MAX_DEPTH") {
        Some(expect_positive_int(tokens, pos, "MAX_DEPTH")? as usize)
    } else {
        None
    };
    Ok(Statement::GraphPath {
        src,
        dst,
        table,
        max_depth,
    })
}

fn parse_graph_edges(tokens: &[Token], pos: &mut usize) -> Result<Statement> {
    expect_keyword(tokens, pos, "EDGES")?;
    let table = expect_ident(tokens, pos)?;
    let filter = parse_where_clause(tokens, pos)?;
    Ok(Statement::GraphEdges { table, filter })
}

// -----------------------------------------------------------------------
// GRAPH COMPONENTS <table>
// -----------------------------------------------------------------------

fn parse_graph_components(tokens: &[Token], pos: &mut usize) -> Result<Statement> {
    expect(tokens, pos, &Token::Components)?;
    let table = expect_ident(tokens, pos)?;
    Ok(Statement::GraphComponents { table })
}

fn parse_graph_communities(tokens: &[Token], pos: &mut usize) -> Result<Statement> {
    expect(tokens, pos, &Token::Communities)?;
    let table = expect_ident(tokens, pos)?;

    let mut algorithm = crate::ast::CommunityAlgorithm::default();
    let mut max_iterations: usize = 10;
    let mut resolution: f64 = 1.0;

    // Parse optional keyword clauses: ALGORITHM <name> | MAX_ITER <n> | RESOLUTION <f>
    loop {
        match tokens.get(*pos) {
            Some(Token::Ident(kw)) if kw.to_uppercase() == "ALGORITHM" => {
                *pos += 1;
                let name = expect_ident(tokens, pos)?;
                algorithm = match name.to_uppercase().as_str() {
                    "LOUVAIN" => crate::ast::CommunityAlgorithm::Louvain,
                    "LP" | "LABELPROPAGATION" | "LABEL_PROPAGATION" => {
                        crate::ast::CommunityAlgorithm::LabelPropagation
                    }
                    other => {
                        return Err(Error::Query(format!(
                            "unknown community algorithm '{other}'; expected louvain or lp"
                        )));
                    }
                };
            }
            Some(Token::Ident(kw)) if kw.to_uppercase() == "MAX_ITER" => {
                *pos += 1;
                match tokens.get(*pos) {
                    Some(Token::IntLit(n)) if *n > 0 => {
                        max_iterations = *n as usize;
                        *pos += 1;
                    }
                    Some(t) => {
                        return Err(Error::Query(format!(
                            "MAX_ITER requires a positive integer, got {t:?}"
                        )));
                    }
                    None => {
                        return Err(Error::Query(
                            "unexpected end of input after MAX_ITER".into(),
                        ));
                    }
                }
            }
            Some(Token::Ident(kw)) if kw.to_uppercase() == "RESOLUTION" => {
                *pos += 1;
                match tokens.get(*pos) {
                    Some(Token::FloatLit(f)) => {
                        resolution = *f;
                        *pos += 1;
                    }
                    Some(Token::IntLit(n)) => {
                        resolution = *n as f64;
                        *pos += 1;
                    }
                    Some(t) => {
                        return Err(Error::Query(format!(
                            "RESOLUTION requires a numeric value, got {t:?}"
                        )));
                    }
                    None => {
                        return Err(Error::Query(
                            "unexpected end of input after RESOLUTION".into(),
                        ));
                    }
                }
            }
            _ => break,
        }
    }

    Ok(Statement::GraphCommunities {
        table,
        algorithm,
        max_iterations,
        resolution,
    })
}

// -----------------------------------------------------------------------
// RELATE src->edge_type->dst [SET field = value, ...]
// -----------------------------------------------------------------------

fn parse_relate(tokens: &[Token], pos: &mut usize) -> Result<Statement> {
    expect(tokens, pos, &Token::Relate)?;
    let src = parse_record_ref(tokens, pos)?;
    expect(tokens, pos, &Token::Arrow)?;
    let edge_type = expect_ident(tokens, pos)?;
    expect(tokens, pos, &Token::Arrow)?;
    let dst = parse_record_ref(tokens, pos)?;

    let fields = if matches!(tokens.get(*pos), Some(Token::Set)) {
        *pos += 1;
        parse_set_clause(tokens, pos)?
    } else {
        vec![]
    };

    Ok(Statement::Relate {
        src,
        edge_type,
        dst,
        fields,
    })
}

// -----------------------------------------------------------------------
// ON CONFLICT UPDATE [SET field = value, ...]
// -----------------------------------------------------------------------

fn parse_on_conflict(tokens: &[Token], pos: &mut usize) -> Result<Option<OnConflict>> {
    if !matches!(tokens.get(*pos), Some(Token::On)) {
        return Ok(None);
    }
    *pos += 1;
    expect(tokens, pos, &Token::Conflict)?;
    expect(tokens, pos, &Token::Update)?;

    // Optional SET clause with explicit update fields.
    if matches!(tokens.get(*pos), Some(Token::Set)) {
        *pos += 1;
        let fields = parse_set_clause(tokens, pos)?;
        Ok(Some(OnConflict::UpdateSet(fields)))
    } else {
        Ok(Some(OnConflict::Update))
    }
}

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

fn parse_record_ref(tokens: &[Token], pos: &mut usize) -> Result<RecordRef> {
    let table = expect_ident(tokens, pos)?;
    expect(tokens, pos, &Token::Colon)?;
    let id = expect_ident(tokens, pos)?;
    Ok(RecordRef { table, id })
}

fn parse_set_clause(tokens: &[Token], pos: &mut usize) -> Result<Vec<(String, Literal)>> {
    let mut fields = vec![parse_field_assignment(tokens, pos)?];
    while matches!(tokens.get(*pos), Some(Token::Comma)) {
        *pos += 1;
        fields.push(parse_field_assignment(tokens, pos)?);
    }
    Ok(fields)
}

fn parse_field_assignment(tokens: &[Token], pos: &mut usize) -> Result<(String, Literal)> {
    let name = expect_ident(tokens, pos)?;
    expect(tokens, pos, &Token::Eq)?;
    let value = parse_literal(tokens, pos)?;
    Ok((name, value))
}

fn parse_where_clause(tokens: &[Token], pos: &mut usize) -> Result<Option<WhereClause>> {
    if !matches!(tokens.get(*pos), Some(Token::Where)) {
        return Ok(None);
    }
    *pos += 1;
    Ok(Some(parse_where_expr(tokens, pos)?))
}

/// Parse a WHERE expression with optional AND chaining (right-associative).
fn parse_where_expr(tokens: &[Token], pos: &mut usize) -> Result<WhereClause> {
    let left = parse_comparison(tokens, pos)?;
    if matches!(tokens.get(*pos), Some(Token::And)) {
        *pos += 1;
        let right = parse_where_expr(tokens, pos)?;
        return Ok(WhereClause::And(Box::new(left), Box::new(right)));
    }
    Ok(left)
}

/// Parse a single `field op literal` comparison, or a `field IS [NOT] NONE`
/// null predicate.
fn parse_comparison(tokens: &[Token], pos: &mut usize) -> Result<WhereClause> {
    let field = expect_ident(tokens, pos)?;

    // `field IS [NOT] NONE`
    if matches!(tokens.get(*pos), Some(Token::Is)) {
        *pos += 1;
        let negated = if matches!(tokens.get(*pos), Some(Token::Not)) {
            *pos += 1;
            true
        } else {
            false
        };
        expect(tokens, pos, &Token::None)?;
        return Ok(WhereClause::IsNull { field, negated });
    }

    let op = parse_cmp_op(tokens, pos)?;
    let value = parse_literal(tokens, pos)?;
    Ok(WhereClause::Cmp { field, op, value })
}

/// Parse a comparison operator token.
fn parse_cmp_op(tokens: &[Token], pos: &mut usize) -> Result<CmpOp> {
    match tokens.get(*pos) {
        Some(Token::Eq) => {
            *pos += 1;
            Ok(CmpOp::Eq)
        }
        Some(Token::Ne) => {
            *pos += 1;
            Ok(CmpOp::Ne)
        }
        Some(Token::Gt) => {
            *pos += 1;
            Ok(CmpOp::Gt)
        }
        Some(Token::Lt) => {
            *pos += 1;
            Ok(CmpOp::Lt)
        }
        Some(Token::Gte) => {
            *pos += 1;
            Ok(CmpOp::Gte)
        }
        Some(Token::Lte) => {
            *pos += 1;
            Ok(CmpOp::Lte)
        }
        Some(t) => Err(Error::Query(format!(
            "expected comparison operator (=, !=, >, <, >=, <=), got {t:?}"
        ))),
        None => Err(Error::Query(
            "unexpected end of input, expected comparison operator".into(),
        )),
    }
}

fn parse_literal(tokens: &[Token], pos: &mut usize) -> Result<Literal> {
    match tokens.get(*pos) {
        Some(Token::StringLit(s)) => {
            let v = Literal::String(s.clone());
            *pos += 1;
            Ok(v)
        }
        Some(Token::IntLit(n)) => {
            let v = Literal::Int(*n);
            *pos += 1;
            Ok(v)
        }
        Some(Token::FloatLit(f)) => {
            let v = Literal::Float(*f);
            *pos += 1;
            Ok(v)
        }
        Some(Token::True) => {
            *pos += 1;
            Ok(Literal::Bool(true))
        }
        Some(Token::False) => {
            *pos += 1;
            Ok(Literal::Bool(false))
        }
        Some(Token::None) => {
            *pos += 1;
            Ok(Literal::None)
        }
        Some(Token::LBracket) => parse_array_literal(tokens, pos),
        Some(t) => Err(Error::Query(format!("expected literal, got {t:?}"))),
        None => Err(Error::Query(
            "unexpected end of input, expected literal".into(),
        )),
    }
}

/// Parse an array literal: `[ literal, literal, ... ]` (possibly empty).
fn parse_array_literal(tokens: &[Token], pos: &mut usize) -> Result<Literal> {
    expect(tokens, pos, &Token::LBracket)?;
    let mut items = Vec::new();

    // Empty array.
    if matches!(tokens.get(*pos), Some(Token::RBracket)) {
        *pos += 1;
        return Ok(Literal::Array(items));
    }

    loop {
        items.push(parse_literal(tokens, pos)?);
        match tokens.get(*pos) {
            Some(Token::Comma) => *pos += 1,
            Some(Token::RBracket) => {
                *pos += 1;
                break;
            }
            Some(t) => {
                return Err(Error::Query(format!(
                    "expected ',' or ']' in array literal, got {t:?}"
                )));
            }
            None => {
                return Err(Error::Query("unterminated array literal".into()));
            }
        }
    }

    Ok(Literal::Array(items))
}

fn expect(tokens: &[Token], pos: &mut usize, expected: &Token) -> Result<()> {
    match tokens.get(*pos) {
        Some(t) if std::mem::discriminant(t) == std::mem::discriminant(expected) => {
            *pos += 1;
            Ok(())
        }
        Some(t) => Err(Error::Query(format!("expected {expected:?}, got {t:?}"))),
        None => Err(Error::Query(format!(
            "unexpected end of input, expected {expected:?}"
        ))),
    }
}

fn expect_ident(tokens: &[Token], pos: &mut usize) -> Result<String> {
    match tokens.get(*pos) {
        Some(Token::Ident(s)) => {
            let name = s.clone();
            *pos += 1;
            Ok(name)
        }
        Some(t) => Err(Error::Query(format!("expected identifier, got {t:?}"))),
        None => Err(Error::Query(
            "unexpected end of input, expected identifier".into(),
        )),
    }
}

/// Expect a contextual keyword (a case-insensitive identifier), consuming it.
///
/// Used for DDL keywords that are intentionally *not* reserved tokens, so that
/// words like `INDEX` or `FIELDS` remain usable as ordinary identifiers.
fn expect_keyword(tokens: &[Token], pos: &mut usize, kw: &str) -> Result<()> {
    match tokens.get(*pos) {
        Some(Token::Ident(s)) if s.eq_ignore_ascii_case(kw) => {
            *pos += 1;
            Ok(())
        }
        Some(t) => Err(Error::Query(format!("expected keyword {kw}, got {t:?}"))),
        None => Err(Error::Query(format!(
            "unexpected end of input, expected keyword {kw}"
        ))),
    }
}

/// Consume a contextual keyword if present, returning whether it was consumed.
fn consume_keyword(tokens: &[Token], pos: &mut usize, kw: &str) -> bool {
    if let Some(Token::Ident(s)) = tokens.get(*pos)
        && s.eq_ignore_ascii_case(kw)
    {
        *pos += 1;
        return true;
    }
    false
}

/// Expect a string literal, consuming it.
fn expect_string(tokens: &[Token], pos: &mut usize) -> Result<String> {
    match tokens.get(*pos) {
        Some(Token::StringLit(s)) => {
            let v = s.clone();
            *pos += 1;
            Ok(v)
        }
        Some(t) => Err(Error::Query(format!("expected string literal, got {t:?}"))),
        None => Err(Error::Query(
            "unexpected end of input, expected string literal".into(),
        )),
    }
}

/// Expect a positive integer literal, consuming it. `what` names the clause
/// for error messages.
fn expect_positive_int(tokens: &[Token], pos: &mut usize, what: &str) -> Result<u64> {
    match tokens.get(*pos) {
        Some(Token::IntLit(n)) if *n > 0 => {
            let v = *n as u64;
            *pos += 1;
            Ok(v)
        }
        Some(Token::IntLit(n)) => Err(Error::Query(format!(
            "{what} must be a positive integer, got {n}"
        ))),
        Some(t) => Err(Error::Query(format!(
            "expected positive integer for {what}, got {t:?}"
        ))),
        None => Err(Error::Query(format!(
            "unexpected end of input, expected integer for {what}"
        ))),
    }
}

/// Parse an optional `ORDER BY <field> [ASC|DESC] (, ...)*` clause.
fn parse_order_by(tokens: &[Token], pos: &mut usize) -> Result<Vec<OrderKey>> {
    if !consume_keyword(tokens, pos, "ORDER") {
        return Ok(Vec::new());
    }
    expect_keyword(tokens, pos, "BY")?;
    let mut keys = vec![parse_order_key(tokens, pos)?];
    while matches!(tokens.get(*pos), Some(Token::Comma)) {
        *pos += 1;
        keys.push(parse_order_key(tokens, pos)?);
    }
    Ok(keys)
}

fn parse_order_key(tokens: &[Token], pos: &mut usize) -> Result<OrderKey> {
    let field = expect_ident(tokens, pos)?;
    let descending = if consume_keyword(tokens, pos, "DESC") {
        true
    } else {
        consume_keyword(tokens, pos, "ASC"); // optional, ascending is default
        false
    };
    Ok(OrderKey { field, descending })
}

/// Expect a numeric literal (int or float), consuming it. `what` names the
/// clause for error messages.
fn expect_number(tokens: &[Token], pos: &mut usize, what: &str) -> Result<f64> {
    match tokens.get(*pos) {
        Some(Token::FloatLit(f)) => {
            let v = *f;
            *pos += 1;
            Ok(v)
        }
        Some(Token::IntLit(n)) => {
            let v = *n as f64;
            *pos += 1;
            Ok(v)
        }
        Some(t) => Err(Error::Query(format!(
            "{what} requires a numeric value, got {t:?}"
        ))),
        None => Err(Error::Query(format!(
            "unexpected end of input, expected number for {what}"
        ))),
    }
}

/// Parse an optional `OUTCOME JSON|TOON|CSV` clause.
fn parse_outcome(tokens: &[Token], pos: &mut usize) -> Result<OutcomeFormat> {
    if !matches!(tokens.get(*pos), Some(Token::Outcome)) {
        return Ok(OutcomeFormat::default());
    }
    *pos += 1;
    match tokens.get(*pos) {
        Some(Token::Ident(s)) => {
            let fmt = match s.to_uppercase().as_str() {
                "JSON" => OutcomeFormat::Json,
                "TOON" => OutcomeFormat::Toon,
                "CSV" => OutcomeFormat::Csv,
                other => {
                    return Err(Error::Query(format!(
                        "unknown outcome format '{other}', expected JSON, TOON, or CSV"
                    )));
                }
            };
            *pos += 1;
            Ok(fmt)
        }
        Some(t) => Err(Error::Query(format!(
            "expected outcome format (JSON, TOON, CSV), got {t:?}"
        ))),
        None => Err(Error::Query(
            "unexpected end of input after OUTCOME, expected JSON, TOON, or CSV".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: unwrap a `Query` to its inner `Statement`.
    fn stmt(input: &str) -> Statement {
        parse(input).unwrap().statement
    }

    #[test]
    fn parse_create_with_id() {
        match stmt("CREATE user:alice SET name = 'Alice', age = 30;") {
            Statement::Create {
                table,
                id,
                fields,
                on_conflict,
            } => {
                assert_eq!(table, "user");
                assert_eq!(id, Some("alice".into()));
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0], ("name".into(), Literal::String("Alice".into())));
                assert_eq!(fields[1], ("age".into(), Literal::Int(30)));
                assert_eq!(on_conflict, None);
            }
            _ => panic!("expected Create"),
        }
    }

    #[test]
    fn parse_create_on_conflict_update() {
        match stmt("CREATE user:alice SET name = 'Alice' ON CONFLICT UPDATE;") {
            Statement::Create {
                table,
                id,
                fields,
                on_conflict,
            } => {
                assert_eq!(table, "user");
                assert_eq!(id, Some("alice".into()));
                assert_eq!(fields.len(), 1);
                assert_eq!(on_conflict, Some(OnConflict::Update));
            }
            _ => panic!("expected Create"),
        }
    }

    #[test]
    fn parse_create_on_conflict_update_set() {
        match stmt(
            "CREATE user:alice SET name = 'Alice', age = 30 ON CONFLICT UPDATE SET age = 31;",
        ) {
            Statement::Create {
                on_conflict,
                fields,
                ..
            } => {
                assert_eq!(fields.len(), 2);
                match on_conflict {
                    Some(OnConflict::UpdateSet(update_fields)) => {
                        assert_eq!(update_fields.len(), 1);
                        assert_eq!(update_fields[0], ("age".into(), Literal::Int(31)));
                    }
                    _ => panic!("expected OnConflict::UpdateSet"),
                }
            }
            _ => panic!("expected Create"),
        }
    }

    #[test]
    fn parse_select_star() {
        assert!(matches!(stmt("SELECT * FROM user"), Statement::Select {
            fields: SelectFields::All,
            from: FromTarget::Table(t),
            filter: None,
            order_by: _,
            limit: None,
        } if t == "user"));
    }

    #[test]
    fn parse_select_limit() {
        match stmt("SELECT * FROM user LIMIT 10") {
            Statement::Select {
                fields: SelectFields::All,
                from: FromTarget::Table(table),
                filter: None,
                order_by: _,
                limit: Some(10),
            } => {
                assert_eq!(table, "user");
            }
            _ => panic!("expected Select with LIMIT"),
        }
    }

    #[test]
    fn parse_select_where_limit() {
        match stmt("SELECT name FROM user WHERE age > 25 LIMIT 3") {
            Statement::Select {
                fields: SelectFields::Named(names),
                filter: Some(WhereClause::Cmp { field, op, value }),
                limit: Some(3),
                ..
            } => {
                assert_eq!(names, vec!["name"]);
                assert_eq!(field, "age");
                assert_eq!(op, CmpOp::Gt);
                assert_eq!(value, Literal::Int(25));
            }
            _ => panic!("expected Select with WHERE and LIMIT"),
        }
    }

    #[test]
    fn parse_select_limit_before_outcome() {
        let q = parse("SELECT * FROM user LIMIT 2 OUTCOME CSV").unwrap();
        assert_eq!(q.outcome, OutcomeFormat::Csv);
        match q.statement {
            Statement::Select { limit, .. } => assert_eq!(limit, Some(2)),
            _ => panic!("expected Select"),
        }
    }

    #[test]
    fn parse_select_limit_rejects_zero() {
        assert!(parse("SELECT * FROM user LIMIT 0").is_err());
    }

    #[test]
    fn parse_select_limit_rejects_negative() {
        assert!(parse("SELECT * FROM user LIMIT -1").is_err());
    }

    #[test]
    fn parse_select_point_lookup() {
        match stmt("SELECT name FROM user:alice") {
            Statement::Select {
                fields: SelectFields::Named(f),
                from: FromTarget::Record(r),
                ..
            } => {
                assert_eq!(f, vec!["name"]);
                assert_eq!(r.table, "user");
                assert_eq!(r.id, "alice");
            }
            _ => panic!("expected Select with Record"),
        }
    }

    #[test]
    fn parse_select_where_eq() {
        match stmt("SELECT * FROM user WHERE age = 30") {
            Statement::Select {
                filter:
                    Some(WhereClause::Cmp {
                        field,
                        op: CmpOp::Eq,
                        value,
                    }),
                ..
            } => {
                assert_eq!(field, "age");
                assert_eq!(value, Literal::Int(30));
            }
            _ => panic!("expected WHERE Cmp Eq"),
        }
    }

    #[test]
    fn parse_select_where_gt() {
        match stmt("SELECT * FROM user WHERE age > 25") {
            Statement::Select {
                filter:
                    Some(WhereClause::Cmp {
                        field,
                        op: CmpOp::Gt,
                        value,
                    }),
                ..
            } => {
                assert_eq!(field, "age");
                assert_eq!(value, Literal::Int(25));
            }
            _ => panic!("expected WHERE Cmp Gt"),
        }
    }

    #[test]
    fn parse_select_where_ne_string() {
        match stmt("SELECT * FROM user WHERE name != 'Bob'") {
            Statement::Select {
                filter:
                    Some(WhereClause::Cmp {
                        field,
                        op: CmpOp::Ne,
                        value,
                    }),
                ..
            } => {
                assert_eq!(field, "name");
                assert_eq!(value, Literal::String("Bob".into()));
            }
            _ => panic!("expected WHERE Cmp Ne"),
        }
    }

    #[test]
    fn parse_select_where_and() {
        match stmt("SELECT * FROM user WHERE age >= 20 AND age <= 30") {
            Statement::Select {
                filter: Some(WhereClause::And(left, right)),
                ..
            } => {
                assert!(matches!(*left, WhereClause::Cmp { op: CmpOp::Gte, .. }));
                assert!(matches!(*right, WhereClause::Cmp { op: CmpOp::Lte, .. }));
            }
            _ => panic!("expected WHERE And"),
        }
    }

    #[test]
    fn parse_traversal_single_hop() {
        match stmt("SELECT ->knows->user FROM user:alice") {
            Statement::Select {
                fields: SelectFields::Traversal(chain),
                from: FromTarget::Record(r),
                ..
            } => {
                assert_eq!(chain.hops.len(), 1);
                assert_eq!(chain.hops[0].direction, TraversalDirection::Out);
                assert_eq!(chain.hops[0].edge_type, "knows");
                assert_eq!(chain.hops[0].dest_table, "user");
                assert_eq!(chain.projection, None);
                assert_eq!(r.id, "alice");
            }
            _ => panic!("expected Traversal Select"),
        }
    }

    #[test]
    fn parse_traversal_with_projection() {
        match stmt("SELECT ->knows->user.name FROM user:alice") {
            Statement::Select {
                fields: SelectFields::Traversal(chain),
                ..
            } => {
                assert_eq!(chain.projection, Some("name".into()));
            }
            _ => panic!("expected Traversal Select with projection"),
        }
    }

    #[test]
    fn parse_traversal_incoming() {
        match stmt("SELECT <-likes<-user FROM user:bob") {
            Statement::Select {
                fields: SelectFields::Traversal(chain),
                ..
            } => {
                assert_eq!(chain.hops[0].direction, TraversalDirection::In);
                assert_eq!(chain.hops[0].edge_type, "likes");
                assert_eq!(chain.hops[0].dest_table, "user");
            }
            _ => panic!("expected incoming Traversal"),
        }
    }

    #[test]
    fn parse_traversal_two_hops() {
        match stmt("SELECT ->knows->user->likes->product.title FROM user:alice") {
            Statement::Select {
                fields: SelectFields::Traversal(chain),
                ..
            } => {
                assert_eq!(chain.hops.len(), 2);
                assert_eq!(chain.hops[0].edge_type, "knows");
                assert_eq!(chain.hops[0].dest_table, "user");
                assert_eq!(chain.hops[1].edge_type, "likes");
                assert_eq!(chain.hops[1].dest_table, "product");
                assert_eq!(chain.projection, Some("title".into()));
            }
            _ => panic!("expected two-hop Traversal"),
        }
    }

    #[test]
    fn parse_delete() {
        assert!(
            matches!(stmt("DELETE user:alice;"), Statement::Delete { table, id } if table == "user" && id == "alice")
        );
    }

    #[test]
    fn parse_relate() {
        match stmt("RELATE user:alice->knows->user:bob SET since = 2020;") {
            Statement::Relate {
                src,
                edge_type,
                dst,
                fields,
            } => {
                assert_eq!(src.table, "user");
                assert_eq!(src.id, "alice");
                assert_eq!(edge_type, "knows");
                assert_eq!(dst.table, "user");
                assert_eq!(dst.id, "bob");
                assert_eq!(fields.len(), 1);
            }
            _ => panic!("expected Relate"),
        }
    }

    #[test]
    fn parse_error_on_garbage() {
        assert!(parse("NONSENSE blah").is_err());
    }

    // -- OUTCOME tests -------------------------------------------------------

    #[test]
    fn parse_outcome_default_is_json() {
        let q = parse("SELECT * FROM user").unwrap();
        assert_eq!(q.outcome, OutcomeFormat::Json);
    }

    #[test]
    fn parse_outcome_json_explicit() {
        let q = parse("SELECT * FROM user OUTCOME JSON").unwrap();
        assert_eq!(q.outcome, OutcomeFormat::Json);
    }

    #[test]
    fn parse_outcome_toon() {
        let q = parse("SELECT * FROM user OUTCOME TOON;").unwrap();
        assert_eq!(q.outcome, OutcomeFormat::Toon);
    }

    #[test]
    fn parse_outcome_csv() {
        let q = parse("SELECT * FROM user WHERE age > 25 OUTCOME CSV").unwrap();
        assert_eq!(q.outcome, OutcomeFormat::Csv);
    }

    #[test]
    fn parse_outcome_case_insensitive() {
        let q = parse("SELECT * FROM user outcome csv;").unwrap();
        assert_eq!(q.outcome, OutcomeFormat::Csv);
    }

    #[test]
    fn parse_outcome_unknown_format() {
        assert!(parse("SELECT * FROM user OUTCOME XML").is_err());
    }

    // -- GRAPH COMMUNITIES tests ---------------------------------------------

    #[test]
    fn parse_graph_communities_defaults() {
        match stmt("GRAPH COMMUNITIES calls") {
            Statement::GraphCommunities {
                table,
                algorithm,
                max_iterations,
                resolution,
            } => {
                assert_eq!(table, "calls");
                assert_eq!(algorithm, CommunityAlgorithm::Louvain);
                assert_eq!(max_iterations, 10);
                assert!((resolution - 1.0).abs() < f64::EPSILON);
            }
            _ => panic!("expected GraphCommunities"),
        }
    }

    #[test]
    fn parse_graph_communities_algorithm_lp() {
        match stmt("GRAPH COMMUNITIES calls ALGORITHM lp") {
            Statement::GraphCommunities { algorithm, .. } => {
                assert_eq!(algorithm, CommunityAlgorithm::LabelPropagation);
            }
            _ => panic!("expected GraphCommunities"),
        }
    }

    #[test]
    fn parse_graph_communities_algorithm_louvain_explicit() {
        match stmt("GRAPH COMMUNITIES calls ALGORITHM louvain") {
            Statement::GraphCommunities { algorithm, .. } => {
                assert_eq!(algorithm, CommunityAlgorithm::Louvain);
            }
            _ => panic!("expected GraphCommunities"),
        }
    }

    #[test]
    fn parse_graph_communities_max_iter() {
        match stmt("GRAPH COMMUNITIES calls MAX_ITER 20") {
            Statement::GraphCommunities { max_iterations, .. } => {
                assert_eq!(max_iterations, 20);
            }
            _ => panic!("expected GraphCommunities"),
        }
    }

    #[test]
    fn parse_graph_communities_resolution_float() {
        match stmt("GRAPH COMMUNITIES calls RESOLUTION 0.5") {
            Statement::GraphCommunities { resolution, .. } => {
                assert!((resolution - 0.5).abs() < f64::EPSILON);
            }
            _ => panic!("expected GraphCommunities"),
        }
    }

    #[test]
    fn parse_graph_communities_all_options() {
        match stmt("GRAPH COMMUNITIES calls ALGORITHM lp MAX_ITER 5 RESOLUTION 2") {
            Statement::GraphCommunities {
                table,
                algorithm,
                max_iterations,
                resolution,
            } => {
                assert_eq!(table, "calls");
                assert_eq!(algorithm, CommunityAlgorithm::LabelPropagation);
                assert_eq!(max_iterations, 5);
                assert!((resolution - 2.0).abs() < f64::EPSILON);
            }
            _ => panic!("expected GraphCommunities"),
        }
    }

    #[test]
    fn parse_graph_communities_with_semicolon() {
        assert!(parse("GRAPH COMMUNITIES calls;").is_ok());
    }

    #[test]
    fn parse_graph_communities_unknown_algorithm_errors() {
        assert!(parse("GRAPH COMMUNITIES calls ALGORITHM spectral").is_err());
    }

    #[test]
    fn parse_graph_missing_communities_errors() {
        assert!(parse("GRAPH calls").is_err());
    }

    // -- DEFINE / REMOVE INDEX tests -----------------------------------------

    #[test]
    fn parse_define_index_basic() {
        match stmt("DEFINE INDEX by_age ON user FIELDS age") {
            Statement::DefineIndex {
                name,
                table,
                fields,
                unique,
            } => {
                assert_eq!(name, "by_age");
                assert_eq!(table, "user");
                assert_eq!(fields, vec!["age"]);
                assert!(!unique);
            }
            _ => panic!("expected DefineIndex"),
        }
    }

    #[test]
    fn parse_define_index_on_table_unique_multi_field() {
        match stmt("DEFINE INDEX idx ON TABLE user FIELDS email, tenant UNIQUE;") {
            Statement::DefineIndex {
                name,
                table,
                fields,
                unique,
            } => {
                assert_eq!(name, "idx");
                assert_eq!(table, "user");
                assert_eq!(fields, vec!["email", "tenant"]);
                assert!(unique);
            }
            _ => panic!("expected DefineIndex"),
        }
    }

    #[test]
    fn parse_define_index_case_insensitive_keywords() {
        // Contextual keywords are case-insensitive.
        assert!(matches!(
            stmt("define index by_age on user fields age unique"),
            Statement::DefineIndex { unique: true, .. }
        ));
    }

    #[test]
    fn parse_remove_index() {
        match stmt("REMOVE INDEX by_age ON user") {
            Statement::RemoveIndex { name, table } => {
                assert_eq!(name, "by_age");
                assert_eq!(table, "user");
            }
            _ => panic!("expected RemoveIndex"),
        }
    }

    #[test]
    fn parse_define_index_requires_fields() {
        assert!(parse("DEFINE INDEX by_age ON user").is_err());
    }

    #[test]
    fn keywords_remain_usable_as_identifiers() {
        // `index`, `fields`, `unique` are not reserved, so a column named
        // `unique` still parses as an ordinary WHERE field.
        assert!(matches!(
            stmt("SELECT * FROM user WHERE unique = 1"),
            Statement::Select { .. }
        ));
    }

    // -- FULLTEXT / VECTOR DDL + search verb tests ---------------------------

    #[test]
    fn parse_define_fulltext_default_analyzer() {
        match stmt("DEFINE FULLTEXT INDEX ft ON article FIELDS body") {
            Statement::DefineFulltextIndex {
                name,
                table,
                field,
                analyzer,
            } => {
                assert_eq!(name, "ft");
                assert_eq!(table, "article");
                assert_eq!(field, "body");
                assert_eq!(analyzer, None);
            }
            _ => panic!("expected DefineFulltextIndex"),
        }
    }

    #[test]
    fn parse_define_fulltext_with_analyzer_on_table() {
        match stmt("DEFINE FULLTEXT INDEX ft ON TABLE article FIELDS body ANALYZER english;") {
            Statement::DefineFulltextIndex {
                table,
                field,
                analyzer,
                ..
            } => {
                assert_eq!(table, "article");
                assert_eq!(field, "body");
                assert_eq!(analyzer, Some("english".into()));
            }
            _ => panic!("expected DefineFulltextIndex"),
        }
    }

    #[test]
    fn parse_define_vector_with_dimension_and_metric() {
        match stmt("DEFINE VECTOR INDEX vec ON doc FIELDS embedding DIMENSION 8 METRIC euclidean") {
            Statement::DefineVectorIndex {
                name,
                table,
                field,
                dim,
                metric,
            } => {
                assert_eq!(name, "vec");
                assert_eq!(table, "doc");
                assert_eq!(field, "embedding");
                assert_eq!(dim, 8);
                assert_eq!(metric, Some("euclidean".into()));
            }
            _ => panic!("expected DefineVectorIndex"),
        }
    }

    #[test]
    fn parse_define_vector_default_metric() {
        match stmt("DEFINE VECTOR INDEX vec ON doc FIELDS embedding DIMENSION 4") {
            Statement::DefineVectorIndex { dim, metric, .. } => {
                assert_eq!(dim, 4);
                assert_eq!(metric, None);
            }
            _ => panic!("expected DefineVectorIndex"),
        }
    }

    #[test]
    fn parse_define_vector_rejects_zero_dimension() {
        assert!(parse("DEFINE VECTOR INDEX vec ON doc FIELDS embedding DIMENSION 0").is_err());
    }

    #[test]
    fn parse_search_with_limit() {
        match stmt("SEARCH article body 'graph database' LIMIT 5") {
            Statement::Search {
                table,
                field,
                query,
                filter: None,
                limit,
            } => {
                assert_eq!(table, "article");
                assert_eq!(field, "body");
                assert_eq!(query, "graph database");
                assert_eq!(limit, Some(5));
            }
            _ => panic!("expected Search"),
        }
    }

    #[test]
    fn parse_search_without_limit() {
        match stmt("SEARCH article body 'hello'") {
            Statement::Search { query, limit, .. } => {
                assert_eq!(query, "hello");
                assert_eq!(limit, None);
            }
            _ => panic!("expected Search"),
        }
    }

    #[test]
    fn parse_vector_search_with_k() {
        match stmt("VECTOR SEARCH doc embedding [0.1, 2, 3.5] K 5") {
            Statement::VectorSearch {
                table,
                field,
                vector,
                filter: None,
                k,
            } => {
                assert_eq!(table, "doc");
                assert_eq!(field, "embedding");
                // Mixed int/float literals coerce to f64.
                assert!(matches!(vector.as_slice(), [_, _, _]));
                assert!((vector[1] - 2.0).abs() < f64::EPSILON);
                assert_eq!(k, Some(5));
            }
            _ => panic!("expected VectorSearch"),
        }
    }

    #[test]
    fn parse_vector_search_without_k() {
        match stmt("VECTOR SEARCH doc embedding [1.0, 2.0]") {
            Statement::VectorSearch { vector, k, .. } => {
                assert!(matches!(vector.as_slice(), [_, _]));
                assert_eq!(k, None);
            }
            _ => panic!("expected VectorSearch"),
        }
    }

    #[test]
    fn parse_vector_search_requires_array() {
        assert!(parse("VECTOR SEARCH doc embedding 'oops'").is_err());
    }

    #[test]
    fn search_and_vector_remain_usable_as_field_names() {
        // `SEARCH` / `VECTOR` are contextual statement keywords only, so they
        // remain valid identifiers inside an ordinary SELECT.
        assert!(matches!(
            stmt("SELECT * FROM doc WHERE search = 1"),
            Statement::Select { .. }
        ));
        assert!(matches!(
            stmt("SELECT * FROM doc WHERE vector = 1"),
            Statement::Select { .. }
        ));
    }

    // -- ORDER BY / GROUP BY / DELETE WHERE / graph verbs / hybrid ------------

    #[test]
    fn parse_select_order_by_desc_then_limit() {
        match stmt("SELECT * FROM user WHERE age > 1 ORDER BY age DESC, name LIMIT 5") {
            Statement::Select {
                order_by, limit, ..
            } => {
                assert!(matches!(order_by.as_slice(), [_, _]));
                assert_eq!(order_by[0].field, "age");
                assert!(order_by[0].descending);
                assert_eq!(order_by[1].field, "name");
                assert!(!order_by[1].descending);
                assert_eq!(limit, Some(5));
            }
            _ => panic!("expected Select"),
        }
    }

    #[test]
    fn parse_select_order_by_defaults_ascending() {
        match stmt("SELECT * FROM user ORDER BY age") {
            Statement::Select { order_by, .. } => {
                assert!(matches!(order_by.as_slice(), [_]));
                assert!(!order_by[0].descending);
            }
            _ => panic!("expected Select"),
        }
    }

    #[test]
    fn parse_select_without_order_is_empty() {
        match stmt("SELECT * FROM user") {
            Statement::Select { order_by, .. } => assert!(order_by.is_empty()),
            _ => panic!("expected Select"),
        }
    }

    #[test]
    fn parse_count_group_by() {
        match stmt("COUNT node WHERE kind = 'fn' GROUP BY lang") {
            Statement::Count {
                table,
                filter,
                group_by,
            } => {
                assert_eq!(table, "node");
                assert!(filter.is_some());
                assert_eq!(group_by, Some("lang".into()));
            }
            _ => panic!("expected Count"),
        }
    }

    #[test]
    fn parse_count_plain_has_no_group() {
        assert!(matches!(
            stmt("COUNT node"),
            Statement::Count { group_by: None, .. }
        ));
    }

    #[test]
    fn parse_delete_point_lookup() {
        match stmt("DELETE user:alice") {
            Statement::Delete { table, id } => {
                assert_eq!(table, "user");
                assert_eq!(id, "alice");
            }
            _ => panic!("expected Delete"),
        }
    }

    #[test]
    fn parse_delete_where() {
        match stmt("DELETE node WHERE file = 'a.rs'") {
            Statement::DeleteWhere { table, filter } => {
                assert_eq!(table, "node");
                assert!(filter.is_some());
            }
            _ => panic!("expected DeleteWhere"),
        }
    }

    #[test]
    fn parse_delete_all_no_filter() {
        assert!(matches!(
            stmt("DELETE node"),
            Statement::DeleteWhere { filter: None, .. }
        ));
    }

    #[test]
    fn parse_search_with_where_and_limit() {
        assert!(matches!(
            stmt("SEARCH article body 'graph' WHERE lang = 'en' LIMIT 5"),
            Statement::Search {
                filter: Some(_),
                limit: Some(5),
                ..
            }
        ));
    }

    #[test]
    fn parse_vector_search_with_where() {
        assert!(matches!(
            stmt("VECTOR SEARCH doc emb [1.0, 2.0] WHERE project = 'p' K 3"),
            Statement::VectorSearch {
                filter: Some(_),
                k: Some(3),
                ..
            }
        ));
    }

    #[test]
    fn parse_graph_pagerank_options() {
        match stmt("GRAPH PAGERANK calls DAMPING 0.9 MAX_ITER 50 LIMIT 10") {
            Statement::GraphPagerank {
                table,
                damping,
                max_iterations,
                limit,
            } => {
                assert_eq!(table, "calls");
                assert!((damping - 0.9).abs() < f64::EPSILON);
                assert_eq!(max_iterations, 50);
                assert_eq!(limit, Some(10));
            }
            _ => panic!("expected GraphPagerank"),
        }
    }

    #[test]
    fn parse_graph_pagerank_defaults() {
        match stmt("GRAPH PAGERANK calls") {
            Statement::GraphPagerank {
                damping,
                max_iterations,
                limit,
                ..
            } => {
                assert!((damping - 0.85).abs() < f64::EPSILON);
                assert_eq!(max_iterations, 100);
                assert_eq!(limit, None);
            }
            _ => panic!("expected GraphPagerank"),
        }
    }

    #[test]
    fn parse_graph_centrality_modes() {
        assert!(matches!(
            stmt("GRAPH CENTRALITY calls"),
            Statement::GraphCentrality {
                mode: CentralityMode::Degree,
                ..
            }
        ));
        assert!(matches!(
            stmt("GRAPH CENTRALITY calls INDEGREE"),
            Statement::GraphCentrality {
                mode: CentralityMode::InDegree,
                ..
            }
        ));
        assert!(matches!(
            stmt("GRAPH CENTRALITY calls OUTDEGREE LIMIT 3"),
            Statement::GraphCentrality {
                mode: CentralityMode::OutDegree,
                limit: Some(3),
                ..
            }
        ));
    }

    #[test]
    fn parse_graph_path_stmt() {
        match stmt("GRAPH PATH a -> b ON calls MAX_DEPTH 4") {
            Statement::GraphPath {
                src,
                dst,
                table,
                max_depth,
            } => {
                assert_eq!(src, "a");
                assert_eq!(dst, "b");
                assert_eq!(table, "calls");
                assert_eq!(max_depth, Some(4));
            }
            _ => panic!("expected GraphPath"),
        }
    }

    #[test]
    fn parse_graph_edges_with_where() {
        match stmt("GRAPH EDGES calls WHERE weight > 0.5") {
            Statement::GraphEdges {
                table,
                filter: Some(_),
            } => assert_eq!(table, "calls"),
            _ => panic!("expected GraphEdges"),
        }
    }

    #[test]
    fn parse_hybrid_search_full() {
        match stmt(
            "HYBRID SEARCH doc TEXT body 'graph db' VECTOR emb [0.1, 0.2] ALPHA 0.7 WHERE lang = 'en' LIMIT 5",
        ) {
            Statement::HybridSearch {
                table,
                text_field,
                query,
                vector_field,
                vector,
                alpha,
                filter,
                limit,
            } => {
                assert_eq!(table, "doc");
                assert_eq!(text_field, "body");
                assert_eq!(query, "graph db");
                assert_eq!(vector_field, "emb");
                assert!(matches!(vector.as_slice(), [_, _]));
                assert_eq!(alpha, Some(0.7));
                assert!(filter.is_some());
                assert_eq!(limit, Some(5));
            }
            _ => panic!("expected HybridSearch"),
        }
    }

    #[test]
    fn parse_hybrid_search_defaults() {
        assert!(matches!(
            stmt("HYBRID SEARCH doc TEXT body 'x' VECTOR emb [1.0]"),
            Statement::HybridSearch {
                alpha: None,
                filter: None,
                limit: None,
                ..
            }
        ));
    }
}
