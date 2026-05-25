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
        Some(t) => Err(Error::Query(format!("expected statement, got {t:?}"))),
        None => Err(Error::Query("empty input".into())),
    }
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
    let limit = parse_limit(tokens, pos)?;

    Ok(Statement::Select {
        fields,
        from,
        filter,
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
    expect(tokens, pos, &Token::Colon)?;
    let id = expect_ident(tokens, pos)?;
    Ok(Statement::Delete { table, id })
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

/// Parse a single `field op literal` comparison.
fn parse_comparison(tokens: &[Token], pos: &mut usize) -> Result<WhereClause> {
    let field = expect_ident(tokens, pos)?;
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
        Some(t) => Err(Error::Query(format!("expected literal, got {t:?}"))),
        None => Err(Error::Query(
            "unexpected end of input, expected literal".into(),
        )),
    }
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
}
