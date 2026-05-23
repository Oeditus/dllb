//! Simple tokenizer for the dllb query language.
//!
//! Splits input into a sequence of [`Token`]s consumed by the parser.

use dllb_core::{Error, Result};

/// A token produced by the tokenizer.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Keywords
    Create,
    Select,
    Delete,
    Relate,
    From,
    Where,
    Set,
    And,
    // Literals
    Ident(String),
    StringLit(String),
    IntLit(i64),
    FloatLit(f64),
    True,
    False,
    None,
    // Symbols
    Star,      // *
    Comma,     // ,
    Semicolon, // ;
    Colon,     // :
    Eq,        // =
    Ne,        // !=
    Gt,        // >
    Lt,        // <
    Gte,       // >=
    Lte,       // <=
    Arrow,     // ->
    BackArrow, // <-
    Dot,       // .
}

/// Tokenize an input string into a list of tokens.
pub fn tokenize(input: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        // Skip whitespace.
        if c.is_whitespace() {
            i += 1;
            continue;
        }

        // Skip line comments.
        if c == '-'
            && i + 1 < chars.len()
            && chars[i + 1] == '-'
            && !(i + 2 < chars.len() && chars[i + 2] == '>')
        {
            // It's a comment (-- but not ->)
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // Operators that require a two-character lookahead.
        // Order matters: longer matches must be checked before shorter ones.

        // BackArrow <-
        if c == '<' && i + 1 < chars.len() && chars[i + 1] == '-' {
            tokens.push(Token::BackArrow);
            i += 2;
            continue;
        }

        // Lte <=
        if c == '<' && i + 1 < chars.len() && chars[i + 1] == '=' {
            tokens.push(Token::Lte);
            i += 2;
            continue;
        }

        // Gte >=
        if c == '>' && i + 1 < chars.len() && chars[i + 1] == '=' {
            tokens.push(Token::Gte);
            i += 2;
            continue;
        }

        // Ne !=
        if c == '!' && i + 1 < chars.len() && chars[i + 1] == '=' {
            tokens.push(Token::Ne);
            i += 2;
            continue;
        }

        // Arrow ->
        if c == '-' && i + 1 < chars.len() && chars[i + 1] == '>' {
            tokens.push(Token::Arrow);
            i += 2;
            continue;
        }

        // Single-char symbols.
        match c {
            '*' => {
                tokens.push(Token::Star);
                i += 1;
                continue;
            }
            ',' => {
                tokens.push(Token::Comma);
                i += 1;
                continue;
            }
            ';' => {
                tokens.push(Token::Semicolon);
                i += 1;
                continue;
            }
            ':' => {
                tokens.push(Token::Colon);
                i += 1;
                continue;
            }
            '=' => {
                tokens.push(Token::Eq);
                i += 1;
                continue;
            }
            '.' => {
                tokens.push(Token::Dot);
                i += 1;
                continue;
            }
            '>' => {
                tokens.push(Token::Gt);
                i += 1;
                continue;
            }
            '<' => {
                tokens.push(Token::Lt);
                i += 1;
                continue;
            }
            _ => {}
        }

        // String literal (single- or double-quoted).
        if c == '\'' || c == '"' {
            let quote = c;
            i += 1;
            let start = i;
            while i < chars.len() && chars[i] != quote {
                i += 1;
            }
            if i >= chars.len() {
                return Err(Error::Query("unterminated string literal".into()));
            }
            let s: String = chars[start..i].iter().collect();
            tokens.push(Token::StringLit(s));
            i += 1; // skip closing quote
            continue;
        }

        // Number (int or float).
        if c.is_ascii_digit() || (c == '-' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit())
        {
            let start = i;
            if c == '-' {
                i += 1;
            }
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i < chars.len() && chars[i] == '.' {
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                let s: String = chars[start..i].iter().collect();
                let f: f64 = s
                    .parse()
                    .map_err(|_| Error::Query(format!("invalid float: {s}")))?;
                tokens.push(Token::FloatLit(f));
            } else {
                let s: String = chars[start..i].iter().collect();
                let n: i64 = s
                    .parse()
                    .map_err(|_| Error::Query(format!("invalid integer: {s}")))?;
                tokens.push(Token::IntLit(n));
            }
            continue;
        }

        // Identifier or keyword.
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let token = match word.to_uppercase().as_str() {
                "CREATE" => Token::Create,
                "SELECT" => Token::Select,
                "DELETE" => Token::Delete,
                "RELATE" => Token::Relate,
                "FROM" => Token::From,
                "WHERE" => Token::Where,
                "SET" => Token::Set,
                "AND" => Token::And,
                "TRUE" => Token::True,
                "FALSE" => Token::False,
                "NONE" | "NULL" => Token::None,
                _ => Token::Ident(word),
            };
            tokens.push(token);
            continue;
        }

        return Err(Error::Query(format!("unexpected character: '{c}'")));
    }

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_create() {
        let tokens = tokenize("CREATE user SET name = 'Alice', age = 30;").unwrap();
        assert_eq!(tokens[0], Token::Create);
        assert_eq!(tokens[1], Token::Ident("user".into()));
        assert_eq!(tokens[2], Token::Set);
        assert_eq!(tokens[3], Token::Ident("name".into()));
        assert_eq!(tokens[4], Token::Eq);
        assert_eq!(tokens[5], Token::StringLit("Alice".into()));
        assert_eq!(tokens[6], Token::Comma);
        assert_eq!(tokens[7], Token::Ident("age".into()));
        assert_eq!(tokens[8], Token::Eq);
        assert_eq!(tokens[9], Token::IntLit(30));
        assert_eq!(tokens[10], Token::Semicolon);
    }

    #[test]
    fn tokenize_select_star() {
        let tokens = tokenize("SELECT * FROM user;").unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::Select,
                Token::Star,
                Token::From,
                Token::Ident("user".into()),
                Token::Semicolon,
            ]
        );
    }

    #[test]
    fn tokenize_relate() {
        let tokens = tokenize("RELATE user:alice->knows->user:bob;").unwrap();
        assert_eq!(tokens[0], Token::Relate);
        assert_eq!(tokens[1], Token::Ident("user".into()));
        assert_eq!(tokens[2], Token::Colon);
        assert_eq!(tokens[3], Token::Ident("alice".into()));
        assert_eq!(tokens[4], Token::Arrow);
        assert_eq!(tokens[5], Token::Ident("knows".into()));
        assert_eq!(tokens[6], Token::Arrow);
        assert_eq!(tokens[7], Token::Ident("user".into()));
        assert_eq!(tokens[8], Token::Colon);
        assert_eq!(tokens[9], Token::Ident("bob".into()));
    }

    #[test]
    fn tokenize_double_quoted_string() {
        let tokens = tokenize(r#"CREATE user SET name = "Alice""#).unwrap();
        assert_eq!(tokens[5], Token::StringLit("Alice".into()));
    }

    #[test]
    fn tokenize_unterminated_string() {
        assert!(tokenize("'hello").is_err());
        assert!(tokenize("\"hello").is_err());
    }

    #[test]
    fn tokenize_comparison_operators() {
        let t = tokenize("age != 30").unwrap();
        assert_eq!(
            t,
            vec![Token::Ident("age".into()), Token::Ne, Token::IntLit(30)]
        );

        let t = tokenize("age > 30").unwrap();
        assert_eq!(
            t,
            vec![Token::Ident("age".into()), Token::Gt, Token::IntLit(30)]
        );

        let t = tokenize("age < 30").unwrap();
        assert_eq!(
            t,
            vec![Token::Ident("age".into()), Token::Lt, Token::IntLit(30)]
        );

        let t = tokenize("age >= 30").unwrap();
        assert_eq!(
            t,
            vec![Token::Ident("age".into()), Token::Gte, Token::IntLit(30)]
        );

        let t = tokenize("age <= 30").unwrap();
        assert_eq!(
            t,
            vec![Token::Ident("age".into()), Token::Lte, Token::IntLit(30)]
        );
    }

    #[test]
    fn tokenize_back_arrow() {
        let t = tokenize("<-likes<-user").unwrap();
        assert_eq!(
            t,
            vec![
                Token::BackArrow,
                Token::Ident("likes".into()),
                Token::BackArrow,
                Token::Ident("user".into()),
            ]
        );
    }

    #[test]
    fn tokenize_traversal_chain() {
        // ->knows->user.name
        let t = tokenize("->knows->user.name").unwrap();
        assert_eq!(
            t,
            vec![
                Token::Arrow,
                Token::Ident("knows".into()),
                Token::Arrow,
                Token::Ident("user".into()),
                Token::Dot,
                Token::Ident("name".into()),
            ]
        );
    }
}
