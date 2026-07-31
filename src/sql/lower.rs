use crate::error::{Error, Result};
use crate::query::TableQuery;
use crate::sql::ast::{Expr, Literal, ProjectionList, SelectStatement};

pub(crate) fn lower_select(statement: SelectStatement) -> Result<TableQuery> {
    let mut query = TableQuery::new(statement.table_name);

    if let ProjectionList::Columns(columns) = statement.projections {
        query = query.select_columns(columns);
    }

    if let Some(where_clause) = statement.where_clause {
        query = lower_where_clause(query, where_clause)?;
    }

    Ok(query)
}

fn lower_where_clause(query: TableQuery, where_clause: Expr) -> Result<TableQuery> {
    let Expr::Equal { left, right } = where_clause else {
        return Err(Error::Unsupported(
            "only equality WHERE clauses are supported",
        ));
    };

    let Expr::Identifier(column_name) = *left else {
        return Err(Error::Unsupported(
            "only column equality WHERE clauses are supported",
        ));
    };
    if column_name != "rowid" {
        return Err(Error::Unsupported("only WHERE rowid equality is supported"));
    }

    let Expr::Literal(Literal::Integer(rowid)) = *right else {
        return Err(Error::Unsupported(
            "only integer rowid equality is supported",
        ));
    };

    Ok(query.rowid_eq(rowid))
}
