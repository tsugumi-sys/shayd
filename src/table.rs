use crate::btree::{BtreePage, PageType};
use crate::error::{Error, Result};
use crate::pager::Pager;
use crate::record::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub rowid: i64,
    pub values: Vec<Value>,
}

pub fn scan_table_page(pager: &mut Pager, root_page: u32) -> Result<Vec<Row>> {
    let page = pager.read_page(root_page)?;
    let btree_page = BtreePage::parse(&page)?;
    if btree_page.header().page_type != PageType::TableLeaf {
        return Err(Error::Unsupported(
            "multi-page rowid table scans are not supported",
        ));
    }

    btree_page
        .table_leaf_cells(&page)?
        .into_iter()
        .map(|cell| {
            Ok(Row {
                rowid: cell.rowid,
                values: cell.record.values().to_vec(),
            })
        })
        .collect()
}
