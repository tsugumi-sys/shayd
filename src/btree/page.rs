use crate::error::{Error, Result};
use crate::header::PageSize;
use crate::pager::Page;

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
    pub(super) page_number: u32,
    pub(super) header: BtreePageHeader,
    pub(super) cell_pointers: Vec<u16>,
}

impl BtreePage {
    pub fn parse(page: &Page) -> Result<Self> {
        Self::parse_with_usable_size(page, page.bytes().len())
    }

    pub fn parse_with_usable_size(page: &Page, usable_size: usize) -> Result<Self> {
        let bytes = page.bytes();
        if usable_size > bytes.len() {
            return Err(Error::InvalidBtreePage("usable size exceeds page size"));
        }

        let offset = page.btree_header_offset();
        if offset >= usable_size {
            return Err(Error::InvalidBtreePage(
                "b-tree header offset is out of bounds",
            ));
        }

        let page_type = PageType::try_from(bytes[offset])?;
        let header_size = page_type.header_size();
        let header_end = offset + header_size;
        if header_end > usable_size {
            return Err(Error::truncated(
                "b-tree page header",
                header_end,
                usable_size,
            ));
        }

        let first_freeblock_offset = read_u16(bytes, offset + 1)?;
        let cell_count = read_u16(bytes, offset + 3)?;
        let cell_content_area_offset =
            parse_cell_content_area_offset(read_u16(bytes, offset + 5)?, usable_size)?;
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
        if pointer_array_end > usable_size {
            return Err(Error::truncated(
                "cell pointer array",
                pointer_array_end,
                usable_size,
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
            if cell_offset_usize < cell_content_area_offset || cell_offset_usize >= usable_size {
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

    pub(super) fn ensure_page(&self, page: &Page) -> Result<()> {
        if self.page_number != page.number() {
            return Err(Error::InvalidBtreePage(
                "parsed b-tree page does not match source page",
            ));
        }
        Ok(())
    }
}

fn parse_cell_content_area_offset(raw: u16, usable_size: usize) -> Result<usize> {
    match (raw, usable_size) {
        (0, usable_size) if usable_size == PageSize::MAX as usize => Ok(PageSize::MAX as usize),
        (0, _) => Err(Error::InvalidBtreePage(
            "cell content area offset cannot be zero",
        )),
        (raw, usable_size) if usize::from(raw) <= usable_size => Ok(usize::from(raw)),
        _ => Err(Error::InvalidBtreePage(
            "cell content area offset is out of bounds",
        )),
    }
}

pub(super) fn read_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let end = offset + 2;
    let slice = bytes
        .get(offset..end)
        .ok_or_else(|| Error::truncated("u16", end, bytes.len()))?;
    Ok(u16::from_be_bytes([slice[0], slice[1]]))
}

pub(super) fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
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
        let page = page_with_leaf_header(1, 100, 0x0d, &[400]);
        let btree_page = BtreePage::parse(&page).unwrap();

        assert_eq!(btree_page.header().offset, 100);
        assert_eq!(btree_page.header().size, 8);
        assert_eq!(btree_page.header().cell_count, 1);
        assert_eq!(btree_page.cell_pointers(), &[400]);
    }

    #[test]
    fn applies_page_two_header_offset() {
        let page = page_with_leaf_header(2, 0, 0x0d, &[400]);
        let btree_page = BtreePage::parse(&page).unwrap();

        assert_eq!(btree_page.header().offset, 0);
        assert_eq!(btree_page.cell_pointers(), &[400]);
    }

    #[test]
    fn parses_cell_pointer_array() {
        let page = page_with_leaf_header(2, 0, 0x0d, &[480, 460, 440]);
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

    pub(super) fn page_with_leaf_header(
        number: u32,
        offset: usize,
        page_type: u8,
        cell_offsets: &[u16],
    ) -> Page {
        let mut bytes = vec![0; 512];
        bytes[offset] = page_type;
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
