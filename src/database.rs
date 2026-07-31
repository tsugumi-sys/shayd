use std::path::Path;

use crate::btree::{BtreePage, PageType};
use crate::error::{Error, Result};
use crate::pager::Pager;
use crate::planner::{QueryPlan, plan_table_query};
use crate::query::{QueryResultRow, TableQuery};
use crate::schema::Schema;
use crate::sql::{lower_select, parse_select};
use crate::table::{NamedRow, Row, lookup_rowid, name_rows, scan_table};

#[derive(Debug)]
pub struct Database {
    pager: Pager,
    schema: Schema,
}

#[derive(Debug)]
pub struct ReadTransaction<'a> {
    pager: &'a mut Pager,
    schema: &'a Schema,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let pager = Pager::open(path)?;
        Self::from_pager(pager)
    }

    pub fn open_bytes(bytes: impl Into<Vec<u8>>) -> Result<Self> {
        let pager = Pager::from_bytes(bytes)?;
        Self::from_pager(pager)
    }

    fn from_pager(mut pager: Pager) -> Result<Self> {
        let schema = Schema::load(&mut pager)?;
        Ok(Self { pager, schema })
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    pub fn read_transaction(&mut self) -> Result<ReadTransaction<'_>> {
        Ok(ReadTransaction {
            pager: &mut self.pager,
            schema: &self.schema,
        })
    }

    pub fn scan_table(&mut self, name: &str) -> Result<Vec<Row>> {
        self.read_transaction()?.scan_table(name)
    }

    pub fn scan_table_named(&mut self, name: &str) -> Result<Vec<NamedRow>> {
        self.read_transaction()?.scan_table_named(name)
    }

    pub fn query_table(&self, name: &str) -> TableQuery {
        TableQuery::new(name)
    }

    pub fn execute_table_query(&mut self, query: TableQuery) -> Result<Vec<QueryResultRow>> {
        self.read_transaction()?.execute_table_query(query)
    }

    pub fn execute_sql(&mut self, sql: &str) -> Result<Vec<QueryResultRow>> {
        self.read_transaction()?.execute_sql(sql)
    }
}

impl ReadTransaction<'_> {
    pub fn schema(&self) -> &Schema {
        self.schema
    }

    pub fn scan_table(&mut self, name: &str) -> Result<Vec<Row>> {
        let table = self
            .schema
            .table(name)
            .ok_or(Error::InvalidSchema("table not found"))?;
        let root_page = table
            .root_page
            .ok_or(Error::InvalidSchema("table has no root page"))?;

        scan_table(self.pager, root_page)
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
        let plan = plan_table_query(&query, self.schema)?;
        let rows = self.read_rows_for_plan(&plan)?;
        query.execute(rows)
    }

    pub fn execute_sql(&mut self, sql: &str) -> Result<Vec<QueryResultRow>> {
        let statement = parse_select(sql)?;
        let query = lower_select(statement)?;
        self.execute_table_query(query)
    }

    fn read_rows_for_plan(&mut self, plan: &QueryPlan) -> Result<Vec<NamedRow>> {
        match plan {
            QueryPlan::FullTableScan { table_name } => self.scan_table_named(table_name),
            QueryPlan::RowidLookup { table_name, rowid } => {
                let table_schema = self
                    .schema
                    .table_schema(table_name)
                    .ok_or(Error::InvalidSchema("table schema not found"))?
                    .clone();
                let root_page = self.table_root_page(table_name)?;
                let Some(row) = lookup_rowid(self.pager, root_page, *rowid)? else {
                    return Ok(Vec::new());
                };

                Ok(vec![NamedRow::from_row(row, &table_schema)?])
            }
            QueryPlan::IndexLookup {
                table_name,
                index_name,
                value,
            } => {
                let table_schema = self
                    .schema
                    .table_schema(table_name)
                    .ok_or(Error::InvalidSchema("table schema not found"))?
                    .clone();
                let table_root_page = self.table_root_page(table_name)?;
                let index = self
                    .schema
                    .index(index_name)
                    .ok_or(Error::InvalidSchema("index not found"))?;
                let index_root_page = index
                    .root_page
                    .ok_or(Error::InvalidSchema("index has no root page"))?;
                let index_page = self.pager.read_page(index_root_page)?;
                let usable_size = self.pager.header().usable_space() as usize;
                let index_btree_page = BtreePage::parse_with_usable_size(&index_page, usable_size)?;
                if index_btree_page.header().page_type != PageType::IndexLeaf {
                    return Err(Error::Unsupported(
                        "index interior traversal is not supported",
                    ));
                }

                let rowids = index_btree_page.index_leaf_rowids_for_value(
                    &index_page,
                    usable_size,
                    value,
                )?;
                rowids
                    .into_iter()
                    .filter_map(
                        |rowid| match lookup_rowid(self.pager, table_root_page, rowid) {
                            Ok(Some(row)) => Some(NamedRow::from_row(row, &table_schema)),
                            Ok(None) => None,
                            Err(error) => Some(Err(error)),
                        },
                    )
                    .collect()
            }
        }
    }

    fn table_root_page(&self, table_name: &str) -> Result<u32> {
        self.schema
            .table(table_name)
            .ok_or(Error::InvalidSchema("table not found"))?
            .root_page
            .ok_or(Error::InvalidSchema("table has no root page"))
    }
}
