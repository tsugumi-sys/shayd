use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::header::DatabaseHeader;

#[derive(Debug)]
pub struct Pager {
    path: PathBuf,
    source: Box<dyn PageSource>,
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
        let source = FilePageSource::open(&path)?;
        Self::from_source(path, Box::new(source))
    }

    fn from_source(path: PathBuf, mut source: Box<dyn PageSource>) -> Result<Self> {
        let mut header_bytes = [0; DatabaseHeader::SIZE];
        source.read_exact_at(0, &mut header_bytes)?;
        let header = DatabaseHeader::parse(&header_bytes)?;
        let file_size = source.len()?;
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
            source,
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
        self.source.read_exact_at(offset, &mut bytes)?;
        Ok(Page {
            number: page_number,
            bytes,
        })
    }
}

trait PageSource: std::fmt::Debug {
    fn len(&self) -> Result<u64>;
    fn read_exact_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()>;
}

#[derive(Debug)]
struct FilePageSource {
    file: File,
}

impl FilePageSource {
    fn open(path: &Path) -> Result<Self> {
        Ok(Self {
            file: File::open(path)?,
        })
    }
}

impl PageSource for FilePageSource {
    fn len(&self) -> Result<u64> {
        Ok(self.file.metadata()?.len())
    }

    fn read_exact_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.read_exact(buf)?;
        Ok(())
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
