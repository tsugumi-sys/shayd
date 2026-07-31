use crate::btree::page::{BtreePage, PageType};
use crate::btree::payload::index_local_payload_size;
use crate::error::{Error, Result};
use crate::pager::Page;
use crate::record::{Record, Value};
use crate::varint;

#[derive(Debug, Clone, PartialEq)]
pub struct IndexLeafCell<'a> {
    pub payload_size: usize,
    pub payload: &'a [u8],
    pub record: Record,
}

impl BtreePage {
    pub fn index_leaf_cells<'a>(
        &self,
        page: &'a Page,
        usable_size: usize,
    ) -> Result<Vec<IndexLeafCell<'a>>> {
        self.ensure_page(page)?;
        if self.header.page_type != PageType::IndexLeaf {
            return Err(Error::InvalidBtreePage("expected index leaf page"));
        }

        self.cell_pointers
            .iter()
            .map(|cell_offset| IndexLeafCell::parse(page, usize::from(*cell_offset), usable_size))
            .collect()
    }

    pub fn index_leaf_rowids_for_value(
        &self,
        page: &Page,
        usable_size: usize,
        value: &Value,
    ) -> Result<Vec<i64>> {
        self.index_leaf_cells(page, usable_size)?
            .into_iter()
            .filter_map(|cell| match cell.record.values() {
                [indexed_value, .., Value::Integer(rowid)] if indexed_value == value => {
                    Some(Ok(*rowid))
                }
                [indexed_value, ..] if indexed_value == value => {
                    Some(Err(Error::InvalidBtreePage("index record missing rowid")))
                }
                _ => None,
            })
            .collect()
    }
}

impl<'a> IndexLeafCell<'a> {
    fn parse(page: &'a Page, offset: usize, usable_size: usize) -> Result<Self> {
        let bytes = page.bytes();
        if usable_size > bytes.len() {
            return Err(Error::InvalidBtreePage("usable size exceeds page size"));
        }

        let cell = bytes
            .get(offset..)
            .ok_or_else(|| Error::truncated("index leaf cell", offset + 1, bytes.len()))?;
        let (payload_size, payload_size_len) = varint::decode(cell)?;
        let payload_size = usize::try_from(payload_size)
            .map_err(|_| Error::InvalidBtreePage("payload too large"))?;
        let payload_start = offset
            .checked_add(payload_size_len)
            .ok_or(Error::InvalidBtreePage("index leaf cell offset overflow"))?;
        let local_payload_size = index_local_payload_size(payload_size, usable_size)?;
        let local_payload_end =
            payload_start
                .checked_add(local_payload_size)
                .ok_or(Error::InvalidBtreePage(
                    "index leaf payload offset overflow",
                ))?;
        if local_payload_end > usable_size {
            return Err(Error::InvalidBtreePage("local payload is out of bounds"));
        }
        if local_payload_size < payload_size {
            return Err(Error::Unsupported(
                "overflow index payloads are not supported",
            ));
        }

        let payload = &bytes[payload_start..local_payload_end];
        let record = Record::decode(payload)?;

        Ok(Self {
            payload_size,
            payload,
            record,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_index_leaf_cell_record() {
        let record = Record::new(vec![
            crate::record::Value::Integer(10),
            crate::record::Value::Integer(1),
        ]);
        let payload = record.encode().unwrap();
        let cell_offset = 512 - (1 + payload.len());
        let mut bytes = vec![0; 512];
        bytes[0] = 0x0a;
        bytes[3..5].copy_from_slice(&1_u16.to_be_bytes());
        bytes[5..7].copy_from_slice(&(cell_offset as u16).to_be_bytes());
        bytes[8..10].copy_from_slice(&(cell_offset as u16).to_be_bytes());
        bytes[cell_offset] = payload.len() as u8;
        bytes[cell_offset + 1..cell_offset + 1 + payload.len()].copy_from_slice(&payload);
        let page = Page::new(2, bytes);

        let btree_page = BtreePage::parse(&page).unwrap();
        let cells = btree_page.index_leaf_cells(&page, 512).unwrap();

        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].payload_size, payload.len());
        assert_eq!(cells[0].payload, payload.as_slice());
        assert_eq!(cells[0].record, record);
    }

    #[test]
    fn finds_index_leaf_rowids_by_indexed_value() {
        let first = Record::new(vec![
            crate::record::Value::Integer(10),
            crate::record::Value::Integer(1),
        ])
        .encode()
        .unwrap();
        let second = Record::new(vec![
            crate::record::Value::Integer(20),
            crate::record::Value::Integer(2),
        ])
        .encode()
        .unwrap();
        let second_offset = 512 - (1 + second.len());
        let first_offset = second_offset - (1 + first.len());
        let mut bytes = vec![0; 512];
        bytes[0] = 0x0a;
        bytes[3..5].copy_from_slice(&2_u16.to_be_bytes());
        bytes[5..7].copy_from_slice(&(first_offset as u16).to_be_bytes());
        bytes[8..10].copy_from_slice(&(first_offset as u16).to_be_bytes());
        bytes[10..12].copy_from_slice(&(second_offset as u16).to_be_bytes());
        bytes[first_offset] = first.len() as u8;
        bytes[first_offset + 1..first_offset + 1 + first.len()].copy_from_slice(&first);
        bytes[second_offset] = second.len() as u8;
        bytes[second_offset + 1..second_offset + 1 + second.len()].copy_from_slice(&second);
        let page = Page::new(2, bytes);

        let btree_page = BtreePage::parse(&page).unwrap();
        let rowids = btree_page
            .index_leaf_rowids_for_value(&page, 512, &crate::record::Value::Integer(20))
            .unwrap();

        assert_eq!(rowids, vec![2]);
    }

    #[test]
    fn rejects_overflow_index_payloads() {
        let cell_offset = 450_usize;
        let mut bytes = vec![0; 512];
        bytes[0] = 0x0a;
        bytes[3..5].copy_from_slice(&1_u16.to_be_bytes());
        bytes[5..7].copy_from_slice(&(cell_offset as u16).to_be_bytes());
        bytes[8..10].copy_from_slice(&(cell_offset as u16).to_be_bytes());
        bytes[cell_offset..cell_offset + 2].copy_from_slice(&[0x83, 0x5e]);
        let page = Page::new(2, bytes);

        let btree_page = BtreePage::parse(&page).unwrap();

        assert!(matches!(
            btree_page.index_leaf_cells(&page, 512),
            Err(Error::Unsupported(
                "overflow index payloads are not supported"
            ))
        ));
    }
}
