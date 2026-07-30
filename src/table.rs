use crate::btree::{BtreePage, PageType};
use crate::error::{Error, Result};
use crate::pager::Pager;
use crate::record::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub rowid: i64,
    pub values: Vec<Value>,
}

pub fn scan_table(pager: &mut Pager, root_page: u32) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    scan_table_into(pager, root_page, &mut rows)?;
    Ok(rows)
}

pub fn scan_table_page(pager: &mut Pager, root_page: u32) -> Result<Vec<Row>> {
    scan_table(pager, root_page)
}

fn scan_table_into(pager: &mut Pager, page_number: u32, rows: &mut Vec<Row>) -> Result<()> {
    let page = pager.read_page(page_number)?;
    let btree_page = BtreePage::parse(&page)?;

    match btree_page.header().page_type {
        PageType::TableLeaf => {
            for cell in btree_page.table_leaf_cells(&page)? {
                rows.push(Row {
                    rowid: cell.rowid,
                    values: cell.record.values().to_vec(),
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
