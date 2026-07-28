use crate::error::{Error, Result};

const DATABASE_HEADER_SIZE: usize = 100;
const MAGIC: &[u8; 16] = b"SQLite format 3\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageSize(u32);

impl PageSize {
    pub const MIN: u32 = 512;
    pub const MAX: u32 = 65_536;

    pub fn from_header_value(value: u16) -> Result<Self> {
        let size = match value {
            1 => Self::MAX,
            value => u32::from(value),
        };

        if !(Self::MIN..=Self::MAX).contains(&size) || !size.is_power_of_two() {
            return Err(Error::InvalidPageSize(value));
        }

        Ok(Self(size))
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    Utf8,
    Utf16Le,
    Utf16Be,
}

impl TextEncoding {
    fn parse(value: u32) -> Result<Self> {
        match value {
            0 | 1 => Ok(Self::Utf8),
            2 => Ok(Self::Utf16Le),
            3 => Ok(Self::Utf16Be),
            value => Err(Error::InvalidTextEncoding(value)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseHeader {
    pub page_size: PageSize,
    pub write_version: u8,
    pub read_version: u8,
    pub reserved_space: u8,
    pub max_embedded_payload_fraction: u8,
    pub min_embedded_payload_fraction: u8,
    pub leaf_payload_fraction: u8,
    pub change_counter: u32,
    pub database_size_pages: u32,
    pub freelist_trunk_page: u32,
    pub freelist_pages: u32,
    pub schema_cookie: u32,
    pub schema_format: u32,
    pub default_page_cache_size: i32,
    pub vacuum_largest_root_page: u32,
    pub text_encoding: TextEncoding,
    pub user_version: i32,
    pub incremental_vacuum: bool,
    pub application_id: i32,
    pub version_valid_for: u32,
    pub sqlite_version_number: u32,
}

impl DatabaseHeader {
    pub const SIZE: usize = DATABASE_HEADER_SIZE;

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < Self::SIZE {
            return Err(Error::truncated("database header", Self::SIZE, bytes.len()));
        }

        if &bytes[0..16] != MAGIC {
            return Err(Error::InvalidDatabaseHeader("bad magic"));
        }

        let page_size = PageSize::from_header_value(read_u16(bytes, 16)?)?;
        let write_version = bytes[18];
        let read_version = bytes[19];
        if !matches!(write_version, 1 | 2) {
            return Err(Error::InvalidDatabaseHeader("unsupported write version"));
        }
        if !matches!(read_version, 1 | 2) {
            return Err(Error::InvalidDatabaseHeader("unsupported read version"));
        }

        let max_embedded_payload_fraction = bytes[21];
        let min_embedded_payload_fraction = bytes[22];
        let leaf_payload_fraction = bytes[23];
        if max_embedded_payload_fraction != 64
            || min_embedded_payload_fraction != 32
            || leaf_payload_fraction != 32
        {
            return Err(Error::InvalidDatabaseHeader("invalid payload fractions"));
        }

        let schema_format = read_u32(bytes, 44)?;
        if schema_format > 4 {
            return Err(Error::InvalidDatabaseHeader("unsupported schema format"));
        }

        let text_encoding = TextEncoding::parse(read_u32(bytes, 56)?)?;
        if text_encoding != TextEncoding::Utf8 {
            return Err(Error::InvalidDatabaseHeader(
                "only UTF-8 databases are supported",
            ));
        }

        if bytes[72..92].iter().any(|byte| *byte != 0) {
            return Err(Error::InvalidDatabaseHeader("reserved bytes must be zero"));
        }

        Ok(Self {
            page_size,
            write_version,
            read_version,
            reserved_space: bytes[20],
            max_embedded_payload_fraction,
            min_embedded_payload_fraction,
            leaf_payload_fraction,
            change_counter: read_u32(bytes, 24)?,
            database_size_pages: read_u32(bytes, 28)?,
            freelist_trunk_page: read_u32(bytes, 32)?,
            freelist_pages: read_u32(bytes, 36)?,
            schema_cookie: read_u32(bytes, 40)?,
            schema_format,
            default_page_cache_size: read_i32(bytes, 48)?,
            vacuum_largest_root_page: read_u32(bytes, 52)?,
            text_encoding,
            user_version: read_i32(bytes, 60)?,
            incremental_vacuum: read_u32(bytes, 64)? != 0,
            application_id: read_i32(bytes, 68)?,
            version_valid_for: read_u32(bytes, 92)?,
            sqlite_version_number: read_u32(bytes, 96)?,
        })
    }

    pub fn usable_space(&self) -> u32 {
        self.page_size.get() - u32::from(self.reserved_space)
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

fn read_i32(bytes: &[u8], offset: usize) -> Result<i32> {
    Ok(read_u32(bytes, offset)? as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_header() -> [u8; DatabaseHeader::SIZE] {
        let mut header = [0; DatabaseHeader::SIZE];
        header[0..16].copy_from_slice(MAGIC);
        header[16..18].copy_from_slice(&4096_u16.to_be_bytes());
        header[18] = 1;
        header[19] = 1;
        header[21] = 64;
        header[22] = 32;
        header[23] = 32;
        header[44..48].copy_from_slice(&4_u32.to_be_bytes());
        header[56..60].copy_from_slice(&1_u32.to_be_bytes());
        header
    }

    #[test]
    fn parses_valid_header() {
        let header = DatabaseHeader::parse(&valid_header()).unwrap();
        assert_eq!(header.page_size.get(), 4096);
        assert_eq!(header.schema_format, 4);
        assert_eq!(header.text_encoding, TextEncoding::Utf8);
    }

    #[test]
    fn treats_page_size_one_as_65536() {
        let mut bytes = valid_header();
        bytes[16..18].copy_from_slice(&1_u16.to_be_bytes());
        let header = DatabaseHeader::parse(&bytes).unwrap();
        assert_eq!(header.page_size.get(), 65_536);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut bytes = valid_header();
        bytes[0] = b'x';
        assert!(matches!(
            DatabaseHeader::parse(&bytes),
            Err(Error::InvalidDatabaseHeader("bad magic"))
        ));
    }

    #[test]
    fn rejects_bad_page_size() {
        let mut bytes = valid_header();
        bytes[16..18].copy_from_slice(&1000_u16.to_be_bytes());
        assert!(matches!(
            DatabaseHeader::parse(&bytes),
            Err(Error::InvalidPageSize(1000))
        ));
    }
}
