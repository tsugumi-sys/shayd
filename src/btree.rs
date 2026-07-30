use crate::error::{Error, Result};
use crate::pager::Page;
use crate::record::Record;
use crate::varint;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageType {
    IndexInterior,
    TableInterior,
    IndexLeaf,
    TableLeaf,
}

impl PageType {
    pub const fn header_size(self) -> usize {
        match self {
            Self::IndexInterior | Self::TableInterior => 12,
            Self::IndexLeaf | Self::TableLeaf => 8,
        }
    }
}

impl TryFrom<u8> for PageType {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0x02 => Ok(Self::IndexInterior),
            0x05 => Ok(Self::TableInterior),
            0x0a => Ok(Self::IndexLeaf),
            0x0d => Ok(Self::TableLeaf),
            value => Err(Error::InvalidBtreePageType(value)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtreePageHeader {
    pub page_type: PageType,
    pub first_freeblock_offset: u16,
    pub cell_count: u16,
    pub cell_content_area_offset: usize,
    pub fragmented_free_bytes: u8,
    pub right_most_pointer: Option<u32>,
    pub offset: usize,
    pub size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtreePage {
    page_number: u32,
    header: BtreePageHeader,
    cell_pointers: Vec<u16>,
}

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
    pub fn parse(page: &Page) -> Result<Self> {
        let bytes = page.bytes();
        let offset = page.btree_header_offset();
        if offset >= bytes.len() {
            return Err(Error::InvalidBtreePage(
                "b-tree header offset is out of bounds",
            ));
        }

        let page_type = PageType::try_from(bytes[offset])?;
        let header_size = page_type.header_size();
        let header_end = offset + header_size;
        if header_end > bytes.len() {
            return Err(Error::truncated(
                "b-tree page header",
                header_end,
                bytes.len(),
            ));
        }

        let first_freeblock_offset = read_u16(bytes, offset + 1)?;
        let cell_count = read_u16(bytes, offset + 3)?;
        let cell_content_area_offset =
            parse_cell_content_area_offset(read_u16(bytes, offset + 5)?, bytes.len())?;
        let fragmented_free_bytes = bytes[offset + 7];
        let right_most_pointer =
            if matches!(page_type, PageType::IndexInterior | PageType::TableInterior) {
                Some(read_u32(bytes, offset + 8)?)
            } else {
                None
            };

        let pointer_array_start = header_end;
        let pointer_array_len = usize::from(cell_count) * 2;
        let pointer_array_end = pointer_array_start
            .checked_add(pointer_array_len)
            .ok_or(Error::InvalidBtreePage("cell pointer array is too large"))?;
        if pointer_array_end > bytes.len() {
            return Err(Error::truncated(
                "cell pointer array",
                pointer_array_end,
                bytes.len(),
            ));
        }
        if pointer_array_end > cell_content_area_offset {
            return Err(Error::InvalidBtreePage(
                "cell pointer array overlaps cell content area",
            ));
        }

        let mut cell_pointers = Vec::with_capacity(usize::from(cell_count));
        for index in 0..usize::from(cell_count) {
            let pointer_offset = pointer_array_start + index * 2;
            let cell_offset = read_u16(bytes, pointer_offset)?;
            let cell_offset_usize = usize::from(cell_offset);
            if cell_offset_usize < cell_content_area_offset || cell_offset_usize >= bytes.len() {
                return Err(Error::InvalidBtreePage("cell offset is out of bounds"));
            }
            cell_pointers.push(cell_offset);
        }

        Ok(Self {
            page_number: page.number(),
            header: BtreePageHeader {
                page_type,
                first_freeblock_offset,
                cell_count,
                cell_content_area_offset,
                fragmented_free_bytes,
                right_most_pointer,
                offset,
                size: header_size,
            },
            cell_pointers,
        })
    }

    pub fn page_number(&self) -> u32 {
        self.page_number
    }

    pub fn header(&self) -> &BtreePageHeader {
        &self.header
    }

    pub fn cell_pointers(&self) -> &[u16] {
        &self.cell_pointers
    }

    pub fn table_leaf_cells<'a>(&self, page: &'a Page) -> Result<Vec<TableLeafCell<'a>>> {
        if self.page_number != page.number() {
            return Err(Error::InvalidBtreePage(
                "parsed b-tree page does not match source page",
            ));
        }
        if self.header.page_type != PageType::TableLeaf {
            return Err(Error::InvalidBtreePage("expected table leaf page"));
        }

        self.cell_pointers
            .iter()
            .map(|cell_offset| TableLeafCell::parse(page, usize::from(*cell_offset)))
            .collect()
    }

    pub fn table_leaf_payloads<'a>(
        &self,
        page: &'a Page,
        usable_size: usize,
    ) -> Result<Vec<TableLeafPayload<'a>>> {
        if self.page_number != page.number() {
            return Err(Error::InvalidBtreePage(
                "parsed b-tree page does not match source page",
            ));
        }
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
        if self.page_number != page.number() {
            return Err(Error::InvalidBtreePage(
                "parsed b-tree page does not match source page",
            ));
        }
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
    fn parse(page: &'a Page, offset: usize) -> Result<Self> {
        let payload = TableLeafPayload::parse(page, offset, page.bytes().len())?;
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

fn parse_cell_content_area_offset(raw: u16, page_len: usize) -> Result<usize> {
    match (raw, page_len) {
        (0, 65_536) => Ok(65_536),
        (0, _) => Err(Error::InvalidBtreePage(
            "cell content area offset cannot be zero",
        )),
        (raw, page_len) if usize::from(raw) <= page_len => Ok(usize::from(raw)),
        _ => Err(Error::InvalidBtreePage(
            "cell content area offset is out of bounds",
        )),
    }
}

pub fn table_leaf_local_payload_size(payload_size: usize, usable_size: usize) -> Result<usize> {
    if usable_size < 35 {
        return Err(Error::InvalidBtreePage("usable size is too small"));
    }

    let max_leaf = usable_size - 35;
    let min_leaf = ((usable_size - 12) * 32 / 255)
        .checked_sub(23)
        .ok_or(Error::InvalidBtreePage("usable size is too small"))?;

    if payload_size <= max_leaf {
        return Ok(payload_size);
    }

    let overflow_payload_capacity = usable_size
        .checked_sub(4)
        .ok_or(Error::InvalidBtreePage("usable size is too small"))?;
    let surplus = min_leaf + ((payload_size - min_leaf) % overflow_payload_capacity);
    if surplus <= max_leaf {
        Ok(surplus)
    } else {
        Ok(min_leaf)
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let end = offset + 2;
    let slice = bytes
        .get(offset..end)
        .ok_or_else(|| Error::truncated("u16", end, bytes.len()))?;
    Ok(u16::from_be_bytes([slice[0], slice[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let end = offset + 4;
    let slice = bytes
        .get(offset..end)
        .ok_or_else(|| Error::truncated("u32", end, bytes.len()))?;
    Ok(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_page_type_values() {
        assert_eq!(PageType::try_from(0x02).unwrap(), PageType::IndexInterior);
        assert_eq!(PageType::try_from(0x05).unwrap(), PageType::TableInterior);
        assert_eq!(PageType::try_from(0x0a).unwrap(), PageType::IndexLeaf);
        assert_eq!(PageType::try_from(0x0d).unwrap(), PageType::TableLeaf);
        assert!(matches!(
            PageType::try_from(0xff),
            Err(Error::InvalidBtreePageType(0xff))
        ));
    }

    #[test]
    fn applies_page_one_header_offset() {
        let page = page_with_leaf_header(1, 100, &[400]);
        let btree_page = BtreePage::parse(&page).unwrap();

        assert_eq!(btree_page.header().offset, 100);
        assert_eq!(btree_page.header().size, 8);
        assert_eq!(btree_page.header().cell_count, 1);
        assert_eq!(btree_page.cell_pointers(), &[400]);
    }

    #[test]
    fn applies_page_two_header_offset() {
        let page = page_with_leaf_header(2, 0, &[400]);
        let btree_page = BtreePage::parse(&page).unwrap();

        assert_eq!(btree_page.header().offset, 0);
        assert_eq!(btree_page.cell_pointers(), &[400]);
    }

    #[test]
    fn parses_cell_pointer_array() {
        let page = page_with_leaf_header(2, 0, &[480, 460, 440]);
        let btree_page = BtreePage::parse(&page).unwrap();

        assert_eq!(btree_page.header().cell_count, 3);
        assert_eq!(btree_page.header().cell_content_area_offset, 440);
        assert_eq!(btree_page.cell_pointers(), &[480, 460, 440]);
    }

    #[test]
    fn parses_interior_page_header() {
        let mut bytes = vec![0; 512];
        bytes[0] = 0x05;
        bytes[3..5].copy_from_slice(&1_u16.to_be_bytes());
        bytes[5..7].copy_from_slice(&400_u16.to_be_bytes());
        bytes[8..12].copy_from_slice(&7_u32.to_be_bytes());
        bytes[12..14].copy_from_slice(&400_u16.to_be_bytes());

        let page = Page::new(2, bytes);
        let btree_page = BtreePage::parse(&page).unwrap();

        assert_eq!(btree_page.header().page_type, PageType::TableInterior);
        assert_eq!(btree_page.header().size, 12);
        assert_eq!(btree_page.header().right_most_pointer, Some(7));
    }

    #[test]
    fn rejects_cell_offsets_outside_content_area() {
        let mut bytes = vec![0; 512];
        bytes[0] = 0x0d;
        bytes[3..5].copy_from_slice(&1_u16.to_be_bytes());
        bytes[5..7].copy_from_slice(&400_u16.to_be_bytes());
        bytes[8..10].copy_from_slice(&399_u16.to_be_bytes());
        let page = Page::new(2, bytes);

        assert!(matches!(
            BtreePage::parse(&page),
            Err(Error::InvalidBtreePage("cell offset is out of bounds"))
        ));
    }

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
    fn computes_table_leaf_local_payload_size_like_sqlite() {
        assert_eq!(table_leaf_local_payload_size(477, 512).unwrap(), 477);
        assert_eq!(table_leaf_local_payload_size(478, 512).unwrap(), 39);
        assert_eq!(table_leaf_local_payload_size(545, 512).unwrap(), 39);
        assert_eq!(table_leaf_local_payload_size(985, 512).unwrap(), 477);
        assert_eq!(table_leaf_local_payload_size(986, 512).unwrap(), 39);
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

    fn page_with_leaf_header(number: u32, offset: usize, cell_offsets: &[u16]) -> Page {
        let mut bytes = vec![0; 512];
        bytes[offset] = 0x0d;
        bytes[offset + 3..offset + 5].copy_from_slice(&(cell_offsets.len() as u16).to_be_bytes());
        let content_start = cell_offsets.iter().copied().min().unwrap_or(512);
        bytes[offset + 5..offset + 7].copy_from_slice(&content_start.to_be_bytes());
        for (index, cell_offset) in cell_offsets.iter().enumerate() {
            let pointer_offset = offset + 8 + index * 2;
            bytes[pointer_offset..pointer_offset + 2].copy_from_slice(&cell_offset.to_be_bytes());
        }
        Page::new(number, bytes)
    }
}
