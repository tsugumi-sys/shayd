use crate::btree::page::{BtreePage, PageType, read_u32};
use crate::btree::payload::table_leaf_local_payload_size;
use crate::error::{Error, Result};
use crate::pager::Page;
use crate::record::Record;
use crate::varint;

#[derive(Debug, Clone, PartialEq)]
pub struct TableLeafCell<'a> {
    pub payload_size: usize,
    pub rowid: i64,
    pub payload: &'a [u8],
    pub record: Record,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableLeafPayload<'a> {
    pub payload_size: usize,
    pub rowid: i64,
    pub local_payload: &'a [u8],
    pub first_overflow_page: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableInteriorCell {
    pub left_child_page: u32,
    pub rowid: i64,
}

impl BtreePage {
    pub fn table_leaf_cells<'a>(&self, page: &'a Page) -> Result<Vec<TableLeafCell<'a>>> {
        self.table_leaf_cells_with_usable_size(page, page.bytes().len())
    }

    pub fn table_leaf_cells_with_usable_size<'a>(
        &self,
        page: &'a Page,
        usable_size: usize,
    ) -> Result<Vec<TableLeafCell<'a>>> {
        self.ensure_page(page)?;
        if self.header.page_type != PageType::TableLeaf {
            return Err(Error::InvalidBtreePage("expected table leaf page"));
        }

        self.cell_pointers
            .iter()
            .map(|cell_offset| TableLeafCell::parse(page, usize::from(*cell_offset), usable_size))
            .collect()
    }

    pub fn table_leaf_payloads<'a>(
        &self,
        page: &'a Page,
        usable_size: usize,
    ) -> Result<Vec<TableLeafPayload<'a>>> {
        self.ensure_page(page)?;
        if self.header.page_type != PageType::TableLeaf {
            return Err(Error::InvalidBtreePage("expected table leaf page"));
        }

        self.cell_pointers
            .iter()
            .map(|cell_offset| {
                TableLeafPayload::parse(page, usize::from(*cell_offset), usable_size)
            })
            .collect()
    }

    pub fn table_interior_cells(&self, page: &Page) -> Result<Vec<TableInteriorCell>> {
        self.ensure_page(page)?;
        if self.header.page_type != PageType::TableInterior {
            return Err(Error::InvalidBtreePage("expected table interior page"));
        }

        self.cell_pointers
            .iter()
            .map(|cell_offset| TableInteriorCell::parse(page, usize::from(*cell_offset)))
            .collect()
    }
}

impl<'a> TableLeafCell<'a> {
    fn parse(page: &'a Page, offset: usize, usable_size: usize) -> Result<Self> {
        let payload = TableLeafPayload::parse(page, offset, usable_size)?;
        if payload.first_overflow_page.is_some() {
            return Err(Error::Unsupported(
                "overflow table leaf payloads require a pager",
            ));
        }

        let record = Record::decode(payload.local_payload)?;

        Ok(Self {
            payload_size: payload.payload_size,
            rowid: payload.rowid,
            payload: payload.local_payload,
            record,
        })
    }
}

impl<'a> TableLeafPayload<'a> {
    fn parse(page: &'a Page, offset: usize, usable_size: usize) -> Result<Self> {
        let bytes = page.bytes();
        if usable_size > bytes.len() {
            return Err(Error::InvalidBtreePage("usable size exceeds page size"));
        }

        let cell = bytes
            .get(offset..)
            .ok_or_else(|| Error::truncated("table leaf cell", offset + 1, bytes.len()))?;

        let (payload_size, payload_size_len) = varint::decode(cell)?;
        let payload_size = usize::try_from(payload_size)
            .map_err(|_| Error::InvalidBtreePage("payload too large"))?;
        let rowid_start = payload_size_len;
        let (rowid, rowid_len) = varint::decode(&cell[rowid_start..])?;
        let payload_start = offset
            .checked_add(payload_size_len)
            .and_then(|value| value.checked_add(rowid_len))
            .ok_or(Error::InvalidBtreePage("table leaf cell offset overflow"))?;
        let local_payload_size = table_leaf_local_payload_size(payload_size, usable_size)?;
        let local_payload_end =
            payload_start
                .checked_add(local_payload_size)
                .ok_or(Error::InvalidBtreePage(
                    "table leaf payload offset overflow",
                ))?;
        if local_payload_end > usable_size {
            return Err(Error::InvalidBtreePage("local payload is out of bounds"));
        }

        let first_overflow_page = if local_payload_size < payload_size {
            let overflow_pointer_end = local_payload_end + 4;
            if overflow_pointer_end > usable_size {
                return Err(Error::truncated(
                    "table leaf overflow pointer",
                    overflow_pointer_end,
                    usable_size,
                ));
            }
            let page_number = read_u32(bytes, local_payload_end)?;
            if page_number == 0 {
                return Err(Error::InvalidBtreePage(
                    "first overflow page cannot be zero",
                ));
            }
            Some(page_number)
        } else {
            None
        };

        Ok(Self {
            payload_size,
            rowid: rowid as i64,
            local_payload: &bytes[payload_start..local_payload_end],
            first_overflow_page,
        })
    }
}

impl TableInteriorCell {
    fn parse(page: &Page, offset: usize) -> Result<Self> {
        let bytes = page.bytes();
        let left_child_page = read_u32(bytes, offset)?;
        if left_child_page == 0 {
            return Err(Error::InvalidBtreePage(
                "table interior child page cannot be zero",
            ));
        }

        let key_offset = offset + 4;
        let key = bytes
            .get(key_offset..)
            .ok_or_else(|| Error::truncated("table interior cell", key_offset + 1, bytes.len()))?;
        let (rowid, _) = varint::decode(key)?;

        Ok(Self {
            left_child_page,
            rowid: rowid as i64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_table_leaf_cell_payload_rowid_and_record() {
        let record = Record::new(vec![
            crate::record::Value::Integer(10),
            crate::record::Value::Text("alpha".to_owned()),
        ]);
        let payload = record.encode().unwrap();
        let cell_offset = 512 - (1 + 1 + payload.len());
        let mut bytes = vec![0; 512];
        bytes[0] = 0x0d;
        bytes[3..5].copy_from_slice(&1_u16.to_be_bytes());
        bytes[5..7].copy_from_slice(&(cell_offset as u16).to_be_bytes());
        bytes[8..10].copy_from_slice(&(cell_offset as u16).to_be_bytes());
        bytes[cell_offset] = payload.len() as u8;
        bytes[cell_offset + 1] = 7;
        bytes[cell_offset + 2..cell_offset + 2 + payload.len()].copy_from_slice(&payload);
        let page = Page::new(2, bytes);

        let btree_page = BtreePage::parse(&page).unwrap();
        let cells = btree_page.table_leaf_cells(&page).unwrap();

        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].payload_size, payload.len());
        assert_eq!(cells[0].rowid, 7);
        assert_eq!(cells[0].payload, payload.as_slice());
        assert_eq!(cells[0].record, record);
    }

    #[test]
    fn rejects_overflow_table_leaf_payloads() {
        let cell_offset = 450_usize;
        let mut bytes = vec![0; 512];
        bytes[0] = 0x0d;
        bytes[3..5].copy_from_slice(&1_u16.to_be_bytes());
        bytes[5..7].copy_from_slice(&(cell_offset as u16).to_be_bytes());
        bytes[8..10].copy_from_slice(&(cell_offset as u16).to_be_bytes());
        bytes[cell_offset..cell_offset + 2].copy_from_slice(&[0x83, 0x5e]);
        bytes[cell_offset + 2] = 1;
        bytes[cell_offset + 42..cell_offset + 46].copy_from_slice(&3_u32.to_be_bytes());
        let page = Page::new(2, bytes);

        let btree_page = BtreePage::parse(&page).unwrap();

        assert!(matches!(
            btree_page.table_leaf_cells(&page),
            Err(Error::Unsupported(
                "overflow table leaf payloads require a pager"
            ))
        ));
    }

    #[test]
    fn parses_table_interior_cell_child_and_rowid() {
        let mut bytes = vec![0; 512];
        bytes[0] = 0x05;
        bytes[3..5].copy_from_slice(&1_u16.to_be_bytes());
        bytes[5..7].copy_from_slice(&500_u16.to_be_bytes());
        bytes[8..12].copy_from_slice(&9_u32.to_be_bytes());
        bytes[12..14].copy_from_slice(&500_u16.to_be_bytes());
        bytes[500..504].copy_from_slice(&3_u32.to_be_bytes());
        bytes[504] = 42;
        let page = Page::new(2, bytes);

        let btree_page = BtreePage::parse(&page).unwrap();
        let cells = btree_page.table_interior_cells(&page).unwrap();

        assert_eq!(btree_page.header().right_most_pointer, Some(9));
        assert_eq!(
            cells,
            vec![TableInteriorCell {
                left_child_page: 3,
                rowid: 42
            }]
        );
    }
}
