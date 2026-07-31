use crate::error::{Error, Result};
use crate::query::TableQuery;
use crate::record::Value;
use crate::schema::Schema;

#[derive(Debug, Clone, PartialEq)]
pub enum QueryPlan {
    FullTableScan {
        table_name: String,
    },
    RowidLookup {
        table_name: String,
        rowid: i64,
    },
    IndexLookup {
        table_name: String,
        index_name: String,
        value: Value,
    },
}

pub fn plan_table_query(query: &TableQuery, schema: &Schema) -> Result<QueryPlan> {
    let table_name = query.table_name();
    let _table_schema = schema
        .table_schema(table_name)
        .ok_or(Error::InvalidSchema("table schema not found"))?;

    let Some(filter) = query.equality_filter() else {
        return Ok(QueryPlan::FullTableScan {
            table_name: table_name.to_owned(),
        });
    };

    if filter.column_name() == "rowid" {
        let Value::Integer(rowid) = filter.value() else {
            return Err(Error::Unsupported(
                "only integer rowid equality is supported",
            ));
        };

        return Ok(QueryPlan::RowidLookup {
            table_name: table_name.to_owned(),
            rowid: *rowid,
        });
    }

    if let Some(index) = schema.index_for_table_column(table_name, filter.column_name()) {
        return Ok(QueryPlan::IndexLookup {
            table_name: table_name.to_owned(),
            index_name: index.name.clone(),
            value: filter.value().clone(),
        });
    }

    Ok(QueryPlan::FullTableScan {
        table_name: table_name.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pager::Pager;

    #[test]
    fn plans_full_table_scan_without_filter() {
        let schema = load_schema(include_bytes!("../tests/fixtures/simple.db"));
        let query = TableQuery::new("t");

        assert_eq!(
            plan_table_query(&query, &schema).unwrap(),
            QueryPlan::FullTableScan {
                table_name: "t".to_owned()
            }
        );
    }

    #[test]
    fn plans_rowid_lookup_for_rowid_equality_filter() {
        let schema = load_schema(include_bytes!("../tests/fixtures/simple.db"));
        let query = TableQuery::new("t").rowid_eq(2);

        assert_eq!(
            plan_table_query(&query, &schema).unwrap(),
            QueryPlan::RowidLookup {
                table_name: "t".to_owned(),
                rowid: 2
            }
        );
    }

    #[test]
    fn plans_index_lookup_for_indexed_column_equality_filter() {
        let schema = load_schema(include_bytes!("../tests/fixtures/indexed.db"));
        let query = TableQuery::new("items").column_eq("a", Value::Integer(20));

        assert_eq!(
            plan_table_query(&query, &schema).unwrap(),
            QueryPlan::IndexLookup {
                table_name: "items".to_owned(),
                index_name: "idx_items_a".to_owned(),
                value: Value::Integer(20)
            }
        );
    }

    #[test]
    fn falls_back_to_full_table_scan_when_column_has_no_index() {
        let schema = load_schema(include_bytes!("../tests/fixtures/simple.db"));
        let query = TableQuery::new("t").column_eq("a", Value::Integer(20));

        assert_eq!(
            plan_table_query(&query, &schema).unwrap(),
            QueryPlan::FullTableScan {
                table_name: "t".to_owned()
            }
        );
    }

    fn load_schema(bytes: &[u8]) -> Schema {
        let mut pager = Pager::from_bytes(bytes.to_vec()).unwrap();
        Schema::load(&mut pager).unwrap()
    }
}
