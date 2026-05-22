//! Recursive-descent parser for the dllb query language.
//!
//! Parses a token stream into an [`Statement`] AST node.

use dllb_core::{Error, Result};

use crate::ast::*;
use crate::tokenizer::{Token, tokenize};

/// Parse an input string into a [`Statement`].
pub fn parse(input: &str) -> Result<Statement> {
    let tokens = tokenize(input)?;
    let mut pos = 0;
    let stmt = parse_statement(&tokens, &mut pos)?;
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
    Ok(stmt)
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

    Ok(Statement::Create { table, id, fields })
}

// -----------------------------------------------------------------------
// SELECT fields FROM target [WHERE clause]
// -----------------------------------------------------------------------

fn parse_select(tokens: &[Token], pos: &mut usize) -> Result<Statement> {
    expect(tokens, pos, &Token::Select)?;

    let fields = if matches!(tokens.get(*pos), Some(Token::Star)) {
        *pos += 1;
        SelectFields::All
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

    Ok(Statement::Select {
        fields,
        from,
        filter,
    })
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
    let field = expect_ident(tokens, pos)?;
    expect(tokens, pos, &Token::Eq)?;
    let value = parse_literal(tokens, pos)?;
    Ok(Some(WhereClause::Eq { field, value }))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_create_with_id() {
        let stmt = parse("CREATE user:alice SET name = 'Alice', age = 30;").unwrap();
        match stmt {
            Statement::Create { table, id, fields } => {
                assert_eq!(table, "user");
                assert_eq!(id, Some("alice".into()));
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0], ("name".into(), Literal::String("Alice".into())));
                assert_eq!(fields[1], ("age".into(), Literal::Int(30)));
            }
            _ => panic!("expected Create"),
        }
    }

    #[test]
    fn parse_select_star() {
        let stmt = parse("SELECT * FROM user").unwrap();
        assert!(matches!(stmt, Statement::Select {
            fields: SelectFields::All,
            from: FromTarget::Table(t),
            filter: None,
        } if t == "user"));
    }

    #[test]
    fn parse_select_point_lookup() {
        let stmt = parse("SELECT name FROM user:alice").unwrap();
        match stmt {
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
    fn parse_select_where() {
        let stmt = parse("SELECT * FROM user WHERE age = 30").unwrap();
        match stmt {
            Statement::Select {
                filter: Some(WhereClause::Eq { field, value }),
                ..
            } => {
                assert_eq!(field, "age");
                assert_eq!(value, Literal::Int(30));
            }
            _ => panic!("expected WHERE clause"),
        }
    }

    #[test]
    fn parse_delete() {
        let stmt = parse("DELETE user:alice;").unwrap();
        assert!(
            matches!(stmt, Statement::Delete { table, id } if table == "user" && id == "alice")
        );
    }

    #[test]
    fn parse_relate() {
        let stmt = parse("RELATE user:alice->knows->user:bob SET since = 2020;").unwrap();
        match stmt {
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
}
