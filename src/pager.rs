use std::collections::HashMap;
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
    page_cache: HashMap<u32, Page>,
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

    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Result<Self> {
        Self::from_source(
            PathBuf::from("<memory>"),
            Box::new(MemoryPageSource::new(bytes)),
        )
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
            page_cache: HashMap::new(),
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
        if let Some(page) = self.page_cache.get(&page_number) {
            return Ok(page.clone());
        }

        let page_size = self.header.page_size.get() as usize;
        let offset = u64::from(page_number - 1) * page_size as u64;
        let mut bytes = vec![0; page_size];
        self.source.read_exact_at(offset, &mut bytes)?;
        let page = Page {
            number: page_number,
            bytes,
        };
        self.page_cache.insert(page_number, page.clone());
        Ok(page)
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::PathBuf;
    use std::rc::Rc;

    use super::*;

    #[test]
    fn repeated_page_reads_use_cache() {
        let read_count = Rc::new(Cell::new(0));
        let source = CountingPageSource {
            bytes: include_bytes!("../tests/fixtures/simple.db").to_vec(),
            read_count: Rc::clone(&read_count),
        };
        let mut pager = Pager::from_source(PathBuf::from("<counting>"), Box::new(source)).unwrap();

        assert_eq!(read_count.get(), 1);

        let first = pager.read_page(1).unwrap();
        let second = pager.read_page(1).unwrap();

        assert_eq!(first, second);
        assert_eq!(read_count.get(), 2);
    }

    #[derive(Debug)]
    struct CountingPageSource {
        bytes: Vec<u8>,
        read_count: Rc<Cell<usize>>,
    }

    impl PageSource for CountingPageSource {
        fn len(&self) -> Result<u64> {
            Ok(self.bytes.len() as u64)
        }

        fn read_exact_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
            self.read_count.set(self.read_count.get() + 1);
            let offset = offset as usize;
            let end = offset + buf.len();
            buf.copy_from_slice(&self.bytes[offset..end]);
            Ok(())
        }
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

#[derive(Debug)]
struct MemoryPageSource {
    bytes: Vec<u8>,
}

impl MemoryPageSource {
    fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            bytes: bytes.into(),
        }
    }
}

impl PageSource for MemoryPageSource {
    fn len(&self) -> Result<u64> {
        u64::try_from(self.bytes.len())
            .map_err(|_| Error::InvalidDatabaseHeader("database image is too large"))
    }

    fn read_exact_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        let offset = usize::try_from(offset)
            .map_err(|_| Error::InvalidDatabaseHeader("database offset is too large"))?;
        let end = offset
            .checked_add(buf.len())
            .ok_or(Error::InvalidDatabaseHeader("database offset is too large"))?;
        let slice = self
            .bytes
            .get(offset..end)
            .ok_or_else(|| Error::truncated("database image", end, self.bytes.len()))?;
        buf.copy_from_slice(slice);
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
