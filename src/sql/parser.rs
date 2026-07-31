use crate::error::{Error, Result};
use crate::sql::ast::{Expr, Literal, ProjectionList, SelectStatement};
use crate::sql::lexer::tokenize;
use crate::sql::token::{Keyword, Token};

pub(crate) fn parse_select(sql: &str) -> Result<SelectStatement> {
    let tokens = tokenize(sql)?;
    let mut parser = Parser::new(&tokens);
    parser.parse_select()
}

struct Parser<'a> {
    tokens: &'a [Token],
    position: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self {
            tokens,
            position: 0,
        }
    }

    fn parse_select(&mut self) -> Result<SelectStatement> {
        self.expect_keyword(Keyword::Select)?;
        let projections = self.parse_projections()?;
        self.expect_keyword(Keyword::From)?;
        let table_name = self.expect_identifier()?;
        let where_clause = if self.consume_keyword(Keyword::Where) {
            Some(self.parse_equal_expr()?)
        } else {
            None
        };

        self.consume_semicolon();
        if self.peek().is_some() {
            return Err(Error::InvalidSql("unexpected token after SELECT statement"));
        }

        Ok(SelectStatement {
            table_name,
            projections,
            where_clause,
        })
    }

    fn parse_projections(&mut self) -> Result<ProjectionList> {
        if self.consume_token(&Token::Star) {
            return Ok(ProjectionList::All);
        }

        let mut columns = vec![self.expect_identifier()?];
        while self.consume_token(&Token::Comma) {
            columns.push(self.expect_identifier()?);
        }

        Ok(ProjectionList::Columns(columns))
    }

    fn parse_equal_expr(&mut self) -> Result<Expr> {
        let left = Expr::Identifier(self.expect_identifier()?);
        self.expect_token(&Token::Equal)?;
        let right = match self.next() {
            Some(Token::Integer(value)) => Expr::Literal(Literal::Integer(*value)),
            Some(Token::String(value)) => Expr::Literal(Literal::String(value.clone())),
            _ => return Err(Error::InvalidSql("expected literal after equals")),
        };

        Ok(Expr::Equal {
            left: Box::new(left),
            right: Box::new(right),
        })
    }

    fn expect_keyword(&mut self, expected: Keyword) -> Result<()> {
        if self.consume_keyword(expected) {
            Ok(())
        } else {
            Err(Error::InvalidSql("expected keyword"))
        }
    }

    fn consume_keyword(&mut self, expected: Keyword) -> bool {
        if self.peek() == Some(&Token::Keyword(expected)) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn expect_identifier(&mut self) -> Result<String> {
        match self.next() {
            Some(Token::Identifier(identifier)) => Ok(identifier.clone()),
            _ => Err(Error::InvalidSql("expected identifier")),
        }
    }

    fn expect_token(&mut self, expected: &Token) -> Result<()> {
        if self.consume_token(expected) {
            Ok(())
        } else {
            Err(Error::InvalidSql("expected token"))
        }
    }

    fn consume_token(&mut self, expected: &Token) -> bool {
        if self.peek() == Some(expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn consume_semicolon(&mut self) {
        let _ = self.consume_token(&Token::Semicolon);
    }

    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.position)
    }

    fn next(&mut self) -> Option<&'a Token> {
        let token = self.peek()?;
        self.position += 1;
        Some(token)
    }
}
