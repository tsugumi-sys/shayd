use std::collections::HashSet;

use crate::btree::{BtreePage, PageType, TableLeafPayload};
use crate::error::{Error, Result};
use crate::pager::Pager;
use crate::record::{Record, Value};
use crate::schema::TableSchema;

const MAX_TABLE_BTREE_DEPTH: usize = 20;

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
    let mut state = ScanState::default();
    scan_table_into(pager, root_page, 0, &mut state)?;
    Ok(state.rows)
}

pub fn lookup_rowid(pager: &mut Pager, root_page: u32, rowid: i64) -> Result<Option<Row>> {
    let mut visited_pages = HashSet::new();
    lookup_rowid_in_page(pager, root_page, rowid, 0, &mut visited_pages)
}

pub fn name_rows(rows: Vec<Row>, table_schema: &TableSchema) -> Result<Vec<NamedRow>> {
    rows.into_iter()
        .map(|row| NamedRow::from_row(row, table_schema))
        .collect()
}

pub fn scan_table_page(pager: &mut Pager, root_page: u32) -> Result<Vec<Row>> {
    scan_table(pager, root_page)
}

#[derive(Debug, Default)]
struct ScanState {
    rows: Vec<Row>,
    visited_pages: HashSet<u32>,
}

fn scan_table_into(
    pager: &mut Pager,
    page_number: u32,
    depth: usize,
    state: &mut ScanState,
) -> Result<()> {
    if depth >= MAX_TABLE_BTREE_DEPTH {
        return Err(Error::InvalidBtreePage("table b-tree depth limit exceeded"));
    }
    if !state.visited_pages.insert(page_number) {
        return Err(Error::InvalidBtreePage("table b-tree cycle detected"));
    }

    let page = pager.read_page(page_number)?;
    let usable_size = pager.header().usable_space() as usize;
    let btree_page = BtreePage::parse_with_usable_size(&page, usable_size)?;

    match btree_page.header().page_type {
        PageType::TableLeaf => {
            for cell in btree_page.table_leaf_payloads(&page, usable_size)? {
                let payload = read_table_leaf_payload(pager, &cell, usable_size)?;
                let record = Record::decode(&payload)?;
                state.rows.push(Row {
                    rowid: cell.rowid,
                    values: record.values().to_vec(),
                });
            }
        }
        PageType::TableInterior => {
            for cell in btree_page.table_interior_cells(&page)? {
                scan_table_into(pager, cell.left_child_page, depth + 1, state)?;
            }
            let right_most_page =
                btree_page
                    .header()
                    .right_most_pointer
                    .ok_or(Error::InvalidBtreePage(
                        "table interior page missing right-most pointer",
                    ))?;
            scan_table_into(pager, right_most_page, depth + 1, state)?;
        }
        PageType::IndexInterior | PageType::IndexLeaf => {
            return Err(Error::InvalidBtreePage("expected table b-tree page"));
        }
    }

    Ok(())
}

fn lookup_rowid_in_page(
    pager: &mut Pager,
    page_number: u32,
    rowid: i64,
    depth: usize,
    visited_pages: &mut HashSet<u32>,
) -> Result<Option<Row>> {
    if depth >= MAX_TABLE_BTREE_DEPTH {
        return Err(Error::InvalidBtreePage("table b-tree depth limit exceeded"));
    }
    if !visited_pages.insert(page_number) {
        return Err(Error::InvalidBtreePage("table b-tree cycle detected"));
    }

    let page = pager.read_page(page_number)?;
    let usable_size = pager.header().usable_space() as usize;
    let btree_page = BtreePage::parse_with_usable_size(&page, usable_size)?;

    match btree_page.header().page_type {
        PageType::TableLeaf => {
            for cell in btree_page.table_leaf_payloads(&page, usable_size)? {
                if cell.rowid == rowid {
                    let payload = read_table_leaf_payload(pager, &cell, usable_size)?;
                    let record = Record::decode(&payload)?;
                    return Ok(Some(Row {
                        rowid: cell.rowid,
                        values: record.values().to_vec(),
                    }));
                }
            }
            Ok(None)
        }
        PageType::TableInterior => {
            let cells = btree_page.table_interior_cells(&page)?;
            let child_page = cells
                .iter()
                .find(|cell| rowid <= cell.rowid)
                .map(|cell| cell.left_child_page)
                .or_else(|| btree_page.header().right_most_pointer)
                .ok_or(Error::InvalidBtreePage(
                    "table interior page missing right-most pointer",
                ))?;

            lookup_rowid_in_page(pager, child_page, rowid, depth + 1, visited_pages)
        }
        PageType::IndexInterior | PageType::IndexLeaf => {
            Err(Error::InvalidBtreePage("expected table b-tree page"))
        }
    }
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
