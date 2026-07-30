use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::header::DatabaseHeader;

#[derive(Debug)]
pub struct Pager {
    path: PathBuf,
    file: File,
    header: DatabaseHeader,
    database_size_pages: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    number: u32,
    bytes: Vec<u8>,
}

impl Pager {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_owned();
        let mut file = File::open(&path)?;
        let mut header_bytes = [0; DatabaseHeader::SIZE];
        file.read_exact(&mut header_bytes)?;
        let header = DatabaseHeader::parse(&header_bytes)?;
        let file_size = file.metadata()?.len();
        let page_size = u64::from(header.page_size.get());
        let file_size_pages = u32::try_from(file_size / page_size)
            .map_err(|_| Error::InvalidDatabaseHeader("database file is too large"))?;
        let header_page_count_is_valid =
            header.database_size_pages != 0 && header.change_counter == header.version_valid_for;
        let database_size_pages = if header_page_count_is_valid {
            if header.database_size_pages > file_size_pages {
                return Err(Error::InvalidDatabaseHeader(
                    "database size exceeds file size",
                ));
            }
            header.database_size_pages
        } else {
            file_size_pages
        };

        Ok(Self {
            path,
            file,
            header,
            database_size_pages,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn header(&self) -> &DatabaseHeader {
        &self.header
    }

    pub fn database_size_pages(&self) -> u32 {
        self.database_size_pages
    }

    pub fn read_page(&mut self, page_number: u32) -> Result<Page> {
        if page_number == 0 {
            return Err(Error::InvalidDatabaseHeader("page numbers start at 1"));
        }
        if page_number > self.database_size_pages {
            return Err(Error::InvalidDatabaseHeader(
                "page number exceeds database size",
            ));
        }

        let page_size = self.header.page_size.get() as usize;
        let offset = u64::from(page_number - 1) * page_size as u64;
        let mut bytes = vec![0; page_size];
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.read_exact(&mut bytes)?;
        Ok(Page {
            number: page_number,
            bytes,
        })
    }
}

impl Page {
    #[cfg(test)]
    pub(crate) fn new(number: u32, bytes: Vec<u8>) -> Self {
        Self { number, bytes }
    }

    pub fn number(&self) -> u32 {
        self.number
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn btree_header_offset(&self) -> usize {
        if self.number == 1 {
            DatabaseHeader::SIZE
        } else {
            0
        }
    }
}
