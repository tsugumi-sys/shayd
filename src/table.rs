use crate::btree::{BtreePage, PageType, TableLeafPayload};
use crate::error::{Error, Result};
use crate::pager::Pager;
use crate::record::{Record, Value};
use crate::schema::TableSchema;

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub rowid: i64,
    pub values: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NamedRow {
    rowid: i64,
    columns: Vec<String>,
    values: Vec<Value>,
}

impl NamedRow {
    pub fn from_row(row: Row, table_schema: &TableSchema) -> Result<Self> {
        if row.values.len() != table_schema.columns.len() {
            return Err(Error::InvalidRow(
                "row value count does not match table column count",
            ));
        }

        Ok(Self {
            rowid: row.rowid,
            columns: table_schema
                .columns
                .iter()
                .map(|column| column.name.clone())
                .collect(),
            values: row.values,
        })
    }

    pub fn rowid(&self) -> i64 {
        self.rowid
    }

    pub fn get(&self, column_name: &str) -> Option<&Value> {
        self.columns
            .iter()
            .position(|column| column == column_name)
            .and_then(|index| self.values.get(index))
    }

    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    pub fn values(&self) -> &[Value] {
        &self.values
    }
}

pub fn scan_table(pager: &mut Pager, root_page: u32) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    scan_table_into(pager, root_page, &mut rows)?;
    Ok(rows)
}

pub fn name_rows(rows: Vec<Row>, table_schema: &TableSchema) -> Result<Vec<NamedRow>> {
    rows.into_iter()
        .map(|row| NamedRow::from_row(row, table_schema))
        .collect()
}

pub fn scan_table_page(pager: &mut Pager, root_page: u32) -> Result<Vec<Row>> {
    scan_table(pager, root_page)
}

fn scan_table_into(pager: &mut Pager, page_number: u32, rows: &mut Vec<Row>) -> Result<()> {
    let page = pager.read_page(page_number)?;
    let btree_page = BtreePage::parse(&page)?;

    match btree_page.header().page_type {
        PageType::TableLeaf => {
            let usable_size = pager.header().usable_space() as usize;
            for cell in btree_page.table_leaf_payloads(&page, usable_size)? {
                let payload = read_table_leaf_payload(pager, &cell, usable_size)?;
                let record = Record::decode(&payload)?;
                rows.push(Row {
                    rowid: cell.rowid,
                    values: record.values().to_vec(),
                });
            }
        }
        PageType::TableInterior => {
            for cell in btree_page.table_interior_cells(&page)? {
                scan_table_into(pager, cell.left_child_page, rows)?;
            }
            let right_most_page =
                btree_page
                    .header()
                    .right_most_pointer
                    .ok_or(Error::InvalidBtreePage(
                        "table interior page missing right-most pointer",
                    ))?;
            scan_table_into(pager, right_most_page, rows)?;
        }
        PageType::IndexInterior | PageType::IndexLeaf => {
            return Err(Error::InvalidBtreePage("expected table b-tree page"));
        }
    }

    Ok(())
}

fn read_table_leaf_payload(
    pager: &mut Pager,
    cell: &TableLeafPayload<'_>,
    usable_size: usize,
) -> Result<Vec<u8>> {
    let mut payload = Vec::with_capacity(cell.payload_size);
    payload.extend_from_slice(cell.local_payload);

    let Some(mut overflow_page_number) = cell.first_overflow_page else {
        return Ok(payload);
    };

    while payload.len() < cell.payload_size {
        validate_overflow_page_number(pager, overflow_page_number)?;
        let overflow_page = pager.read_page(overflow_page_number)?;
        let bytes = overflow_page.bytes();
        if usable_size > bytes.len() || usable_size < 4 {
            return Err(Error::InvalidBtreePage("invalid overflow page usable size"));
        }

        let next_overflow_page = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let remaining = cell.payload_size - payload.len();
        let available = usable_size - 4;
        let take = remaining.min(available);
        payload.extend_from_slice(&bytes[4..4 + take]);

        if payload.len() == cell.payload_size {
            break;
        }
        if next_overflow_page == 0 {
            return Err(Error::InvalidBtreePage("overflow chain ended early"));
        }
        overflow_page_number = next_overflow_page;
    }

    Ok(payload)
}

fn validate_overflow_page_number(pager: &Pager, page_number: u32) -> Result<()> {
    if page_number == 0 {
        return Err(Error::InvalidBtreePage("overflow page cannot be zero"));
    }

    let database_size_pages = pager.header().database_size_pages;
    if database_size_pages != 0 && page_number > database_size_pages {
        return Err(Error::InvalidBtreePage(
            "overflow page exceeds database size",
        ));
    }

    Ok(())
}
