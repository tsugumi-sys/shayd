use std::path::Path;

use crate::error::{Error, Result};
use crate::pager::Pager;
use crate::query::{QueryResultRow, TableQuery};
use crate::schema::Schema;
use crate::table::{NamedRow, Row, name_rows, scan_table};

#[derive(Debug)]
pub struct Database {
    pager: Pager,
    schema: Schema,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut pager = Pager::open(path)?;
        let schema = Schema::load(&mut pager)?;
        Ok(Self { pager, schema })
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    pub fn scan_table(&mut self, name: &str) -> Result<Vec<Row>> {
        let table = self
            .schema
            .table(name)
            .ok_or(Error::InvalidSchema("table not found"))?;
        let root_page = table
            .root_page
            .ok_or(Error::InvalidSchema("table has no root page"))?;

        scan_table(&mut self.pager, root_page)
    }

    pub fn scan_table_named(&mut self, name: &str) -> Result<Vec<NamedRow>> {
        let table_schema = self
            .schema
            .table_schema(name)
            .ok_or(Error::InvalidSchema("table schema not found"))?
            .clone();
        let rows = self.scan_table(name)?;

        name_rows(rows, &table_schema)
    }

    pub fn query_table(&self, name: &str) -> TableQuery {
        TableQuery::new(name)
    }

    pub fn execute_table_query(&mut self, query: TableQuery) -> Result<Vec<QueryResultRow>> {
        let rows = self.scan_table_named(query.table_name())?;
        query.execute(rows)
    }
}
