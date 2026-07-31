use crate::error::{Error, Result};
use crate::sql::ast::SelectStatement;

pub(crate) fn parse_select(_sql: &str) -> Result<SelectStatement> {
    Err(Error::Unsupported("SQL SELECT parser is not implemented"))
}
