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
        Ok(Self { path, file, header })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn header(&self) -> &DatabaseHeader {
        &self.header
    }

    pub fn read_page(&mut self, page_number: u32) -> Result<Page> {
        if page_number == 0 {
            return Err(Error::InvalidDatabaseHeader("page numbers start at 1"));
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
