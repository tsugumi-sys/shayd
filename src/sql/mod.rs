#![allow(dead_code)]

pub(crate) mod ast;
pub(crate) mod lexer;
pub(crate) mod lower;
pub(crate) mod parser;
pub(crate) mod token;

pub(crate) use lower::lower_select;
pub(crate) use parser::parse_select;

#[cfg(test)]
mod tests {
    use super::ast::{Expr, Literal, ProjectionList, SelectStatement};
    use super::lexer::tokenize;
    use super::lower::lower_select;
    use super::parser::parse_select;
    use super::token::{Keyword, Token};
    use crate::error::Error;
    use crate::record::Value;
    use crate::schema::{ColumnSchema, TableSchema};
    use crate::table::{NamedRow, Row};

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
            parse_select("DELETE FROM t"),
            Err(Error::InvalidSql(_))
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

    #[test]
    fn parses_wildcard_projection() {
        assert_eq!(
            parse_select("SELECT * FROM t").unwrap(),
            SelectStatement {
                table_name: "t".to_owned(),
                projections: ProjectionList::All,
                where_clause: None,
            }
        );
    }

    #[test]
    fn parses_named_projection_list() {
        assert_eq!(
            parse_select("SELECT a, b FROM t").unwrap(),
            SelectStatement {
                table_name: "t".to_owned(),
                projections: ProjectionList::Columns(vec!["a".to_owned(), "b".to_owned()]),
                where_clause: None,
            }
        );
    }

    #[test]
    fn parses_where_rowid_equals_integer() {
        assert_eq!(
            parse_select("SELECT rowid, a FROM t WHERE rowid = 1").unwrap(),
            SelectStatement {
                table_name: "t".to_owned(),
                projections: ProjectionList::Columns(vec!["rowid".to_owned(), "a".to_owned()]),
                where_clause: Some(Expr::Equal {
                    left: Box::new(Expr::Identifier("rowid".to_owned())),
                    right: Box::new(Expr::Literal(Literal::Integer(1))),
                }),
            }
        );
    }

    #[test]
    fn parses_optional_trailing_semicolon() {
        assert_eq!(
            parse_select("SELECT * FROM t;").unwrap(),
            SelectStatement {
                table_name: "t".to_owned(),
                projections: ProjectionList::All,
                where_clause: None,
            }
        );
    }

    #[test]
    fn rejects_non_select_statements_and_unsupported_features() {
        assert!(matches!(
            parse_select("DELETE FROM t"),
            Err(Error::InvalidSql("expected keyword"))
        ));
        assert!(matches!(
            parse_select("SELECT * FROM t ORDER BY a"),
            Err(Error::InvalidSql("unexpected token after SELECT statement"))
        ));
        assert!(matches!(
            parse_select("SELECT FROM t"),
            Err(Error::InvalidSql("expected identifier"))
        ));
    }

    #[test]
    fn lowers_wildcard_projection() {
        let query = lower_select(parse_select("SELECT * FROM t").unwrap()).unwrap();
        let rows = query.execute(named_rows()).unwrap();

        assert_eq!(
            rows[0].values(),
            &[
                ("a".to_owned(), Value::Integer(10)),
                ("b".to_owned(), Value::Text("alpha".to_owned())),
            ]
        );
    }

    #[test]
    fn lowers_named_projection_list() {
        let query = lower_select(parse_select("SELECT a, b FROM t").unwrap()).unwrap();
        let rows = query.execute(named_rows()).unwrap();

        assert_eq!(
            rows[0].values(),
            &[
                ("a".to_owned(), Value::Integer(10)),
                ("b".to_owned(), Value::Text("alpha".to_owned())),
            ]
        );
    }

    #[test]
    fn lowers_rowid_equality_filter() {
        let query =
            lower_select(parse_select("SELECT rowid, a FROM t WHERE rowid = 2").unwrap()).unwrap();
        let rows = query.execute(named_rows()).unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].values(),
            &[
                ("rowid".to_owned(), Value::Integer(2)),
                ("a".to_owned(), Value::Integer(20)),
            ]
        );
    }

    #[test]
    fn lowers_column_equality_filter() {
        let query =
            lower_select(parse_select("SELECT rowid, a FROM t WHERE a = 20").unwrap()).unwrap();
        let rows = query.execute(named_rows()).unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].values(),
            &[
                ("rowid".to_owned(), Value::Integer(2)),
                ("a".to_owned(), Value::Integer(20)),
            ]
        );
    }

    #[test]
    fn rejects_unsupported_where_during_lowering() {
        assert!(matches!(
            lower_select(parse_select("SELECT a FROM t WHERE rowid = '2'").unwrap()),
            Err(Error::Unsupported(
                "only integer rowid equality is supported"
            ))
        ));
    }

    fn named_rows() -> Vec<NamedRow> {
        let schema = TableSchema {
            name: "t".to_owned(),
            columns: vec![
                ColumnSchema {
                    name: "a".to_owned(),
                    declared_type: None,
                },
                ColumnSchema {
                    name: "b".to_owned(),
                    declared_type: None,
                },
            ],
        };

        vec![
            NamedRow::from_row(
                Row {
                    rowid: 1,
                    values: vec![Value::Integer(10), Value::Text("alpha".to_owned())],
                },
                &schema,
            )
            .unwrap(),
            NamedRow::from_row(
                Row {
                    rowid: 2,
                    values: vec![Value::Integer(20), Value::Text("beta".to_owned())],
                },
                &schema,
            )
            .unwrap(),
        ]
    }
}
