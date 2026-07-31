#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectStatement {
    pub table_name: String,
    pub projections: ProjectionList,
    pub where_clause: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProjectionList {
    All,
    Columns(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Expr {
    Equal { left: Box<Expr>, right: Box<Expr> },
    Identifier(String),
    Literal(Literal),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Literal {
    Integer(i64),
    String(String),
}
