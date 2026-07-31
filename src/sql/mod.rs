#![allow(dead_code)]

pub(crate) mod ast;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod token;

#[cfg(test)]
mod tests {
    use super::ast::{Expr, Literal, ProjectionList, SelectStatement};
    use super::lexer::tokenize;
    use super::parser::parse_select;
    use super::token::{Keyword, Token};
    use crate::error::Error;

    #[test]
    fn defines_minimal_select_ast_shape() {
        let statement = SelectStatement {
            table_name: "t".to_owned(),
            projections: ProjectionList::Columns(vec!["rowid".to_owned(), "a".to_owned()]),
            where_clause: Some(Expr::Equal {
                left: Box::new(Expr::Identifier("rowid".to_owned())),
                right: Box::new(Expr::Literal(Literal::Integer(1))),
            }),
        };

        assert_eq!(statement.table_name, "t");
        assert_eq!(
            statement.projections,
            ProjectionList::Columns(vec!["rowid".to_owned(), "a".to_owned()])
        );
        assert_eq!(
            statement.where_clause,
            Some(Expr::Equal {
                left: Box::new(Expr::Identifier("rowid".to_owned())),
                right: Box::new(Expr::Literal(Literal::Integer(1))),
            })
        );
    }

    #[test]
    fn defines_initial_token_set() {
        let tokens = vec![
            Token::Keyword(Keyword::Select),
            Token::Star,
            Token::Keyword(Keyword::From),
            Token::Identifier("t".to_owned()),
            Token::Keyword(Keyword::Where),
            Token::Identifier("rowid".to_owned()),
            Token::Equal,
            Token::Integer(1),
            Token::String("one".to_owned()),
            Token::Comma,
            Token::Semicolon,
            Token::LeftParen,
            Token::RightParen,
        ];

        assert_eq!(tokens.len(), 13);
    }

    #[test]
    fn lexer_and_parser_are_stubs_until_next_steps() {
        assert!(matches!(
            parse_select("SELECT * FROM t"),
            Err(Error::Unsupported("SQL SELECT parser is not implemented"))
        ));
    }

    #[test]
    fn tokenizes_basic_select() {
        assert_eq!(
            tokenize("SELECT * FROM t").unwrap(),
            vec![
                Token::Keyword(Keyword::Select),
                Token::Star,
                Token::Keyword(Keyword::From),
                Token::Identifier("t".to_owned()),
            ]
        );
    }

    #[test]
    fn tokenizes_mixed_case_keywords() {
        assert_eq!(
            tokenize("select A FrOm t WhErE rowid = 1").unwrap(),
            vec![
                Token::Keyword(Keyword::Select),
                Token::Identifier("A".to_owned()),
                Token::Keyword(Keyword::From),
                Token::Identifier("t".to_owned()),
                Token::Keyword(Keyword::Where),
                Token::Identifier("rowid".to_owned()),
                Token::Equal,
                Token::Integer(1),
            ]
        );
    }

    #[test]
    fn tokenizes_identifiers_integers_and_strings() {
        assert_eq!(
            tokenize("SELECT a, b FROM t WHERE b = 'it''s ok';").unwrap(),
            vec![
                Token::Keyword(Keyword::Select),
                Token::Identifier("a".to_owned()),
                Token::Comma,
                Token::Identifier("b".to_owned()),
                Token::Keyword(Keyword::From),
                Token::Identifier("t".to_owned()),
                Token::Keyword(Keyword::Where),
                Token::Identifier("b".to_owned()),
                Token::Equal,
                Token::String("it's ok".to_owned()),
                Token::Semicolon,
            ]
        );
    }

    #[test]
    fn skips_whitespace_and_comments() {
        assert_eq!(
            tokenize("SELECT /* columns */ a -- tail\nFROM t").unwrap(),
            vec![
                Token::Keyword(Keyword::Select),
                Token::Identifier("a".to_owned()),
                Token::Keyword(Keyword::From),
                Token::Identifier("t".to_owned()),
            ]
        );
    }

    #[test]
    fn rejects_invalid_or_unterminated_input() {
        assert!(matches!(
            tokenize("SELECT @ FROM t"),
            Err(Error::InvalidSql("invalid character"))
        ));
        assert!(matches!(
            tokenize("SELECT 'abc"),
            Err(Error::InvalidSql("unterminated string literal"))
        ));
        assert!(matches!(
            tokenize("SELECT /* abc"),
            Err(Error::InvalidSql("unterminated block comment"))
        ));
    }
}
