use std::path::Path;

use crate::error::{Error, Result};
use crate::pager::Pager;
use crate::schema::Schema;
use crate::table::{Row, scan_table};

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
}
