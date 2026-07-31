#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Keyword {
    From,
    Select,
    Where,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Token {
    Keyword(Keyword),
    Identifier(String),
    Integer(i64),
    String(String),
    Star,
    Comma,
    Equal,
    Semicolon,
    LeftParen,
    RightParen,
}
