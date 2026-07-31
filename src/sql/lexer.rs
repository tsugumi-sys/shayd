use crate::error::{Error, Result};
use crate::sql::token::{Keyword, Token};

pub(crate) fn tokenize(sql: &str) -> Result<Vec<Token>> {
    let mut lexer = Lexer::new(sql);
    lexer.tokenize()
}

struct Lexer<'a> {
    input: &'a str,
    position: usize,
}

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, position: 0 }
    }

    fn tokenize(&mut self) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();

        while let Some(byte) = self.peek_byte() {
            match byte {
                b' ' | b'\t' | b'\n' | b'\r' | 0x0c => self.position += 1,
                b'-' if self.peek_next_byte() == Some(b'-') => self.skip_line_comment(),
                b'/' if self.peek_next_byte() == Some(b'*') => self.skip_block_comment()?,
                b'a'..=b'z' | b'A'..=b'Z' | b'_' => tokens.push(self.read_word()),
                b'0'..=b'9' => tokens.push(self.read_integer()?),
                b'\'' => tokens.push(self.read_string()?),
                b'*' => {
                    self.position += 1;
                    tokens.push(Token::Star);
                }
                b',' => {
                    self.position += 1;
                    tokens.push(Token::Comma);
                }
                b'=' => {
                    self.position += 1;
                    tokens.push(Token::Equal);
                }
                b';' => {
                    self.position += 1;
                    tokens.push(Token::Semicolon);
                }
                b'(' => {
                    self.position += 1;
                    tokens.push(Token::LeftParen);
                }
                b')' => {
                    self.position += 1;
                    tokens.push(Token::RightParen);
                }
                _ => return Err(Error::InvalidSql("invalid character")),
            }
        }

        Ok(tokens)
    }

    fn peek_byte(&self) -> Option<u8> {
        self.input.as_bytes().get(self.position).copied()
    }

    fn peek_next_byte(&self) -> Option<u8> {
        self.input.as_bytes().get(self.position + 1).copied()
    }

    fn read_word(&mut self) -> Token {
        let start = self.position;
        while self
            .peek_byte()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            self.position += 1;
        }

        let word = &self.input[start..self.position];
        match word.to_ascii_uppercase().as_str() {
            "FROM" => Token::Keyword(Keyword::From),
            "SELECT" => Token::Keyword(Keyword::Select),
            "WHERE" => Token::Keyword(Keyword::Where),
            _ => Token::Identifier(word.to_owned()),
        }
    }

    fn read_integer(&mut self) -> Result<Token> {
        let start = self.position;
        while self.peek_byte().is_some_and(|byte| byte.is_ascii_digit()) {
            self.position += 1;
        }

        let value = self.input[start..self.position]
            .parse()
            .map_err(|_| Error::InvalidSql("integer literal is out of range"))?;
        Ok(Token::Integer(value))
    }

    fn read_string(&mut self) -> Result<Token> {
        self.position += 1;
        let mut value = String::new();

        while let Some(byte) = self.peek_byte() {
            self.position += 1;
            match byte {
                b'\'' if self.peek_byte() == Some(b'\'') => {
                    self.position += 1;
                    value.push('\'');
                }
                b'\'' => return Ok(Token::String(value)),
                _ if byte.is_ascii() => value.push(byte as char),
                _ => {
                    return Err(Error::InvalidSql(
                        "non-ASCII string literal is not supported",
                    ));
                }
            }
        }

        Err(Error::InvalidSql("unterminated string literal"))
    }

    fn skip_line_comment(&mut self) {
        self.position += 2;
        while let Some(byte) = self.peek_byte() {
            self.position += 1;
            if byte == b'\n' {
                break;
            }
        }
    }

    fn skip_block_comment(&mut self) -> Result<()> {
        self.position += 2;
        while let Some(byte) = self.peek_byte() {
            if byte == b'*' && self.peek_next_byte() == Some(b'/') {
                self.position += 2;
                return Ok(());
            }
            self.position += 1;
        }

        Err(Error::InvalidSql("unterminated block comment"))
    }
}
