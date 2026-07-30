use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    InvalidBtreePage(&'static str),
    InvalidBtreePageType(u8),
    InvalidDatabaseHeader(&'static str),
    InvalidPageSize(u16),
    InvalidSchema(&'static str),
    InvalidTextEncoding(u32),
    InvalidVarint,
    ReservedSerialType(u64),
    Truncated {
        context: &'static str,
        needed: usize,
        available: usize,
    },
    Unsupported(&'static str),
    Utf8(std::str::Utf8Error),
}

impl Error {
    pub(crate) fn truncated(context: &'static str, needed: usize, available: usize) -> Self {
        Self::Truncated {
            context,
            needed,
            available,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::InvalidBtreePage(message) => {
                write!(f, "invalid b-tree page: {message}")
            }
            Self::InvalidBtreePageType(page_type) => {
                write!(f, "invalid b-tree page type: 0x{page_type:02x}")
            }
            Self::InvalidDatabaseHeader(message) => {
                write!(f, "invalid database header: {message}")
            }
            Self::InvalidPageSize(size) => write!(f, "invalid database page size: {size}"),
            Self::InvalidSchema(message) => write!(f, "invalid schema: {message}"),
            Self::InvalidTextEncoding(encoding) => {
                write!(f, "unsupported database text encoding: {encoding}")
            }
            Self::InvalidVarint => f.write_str("invalid SQLite varint"),
            Self::ReservedSerialType(serial_type) => {
                write!(f, "reserved SQLite serial type: {serial_type}")
            }
            Self::Truncated {
                context,
                needed,
                available,
            } => write!(
                f,
                "truncated {context}: needed {needed} bytes, available {available}"
            ),
            Self::Unsupported(message) => write!(f, "unsupported SQLite feature: {message}"),
            Self::Utf8(err) => write!(f, "invalid UTF-8 text: {err}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Utf8(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<std::str::Utf8Error> for Error {
    fn from(value: std::str::Utf8Error) -> Self {
        Self::Utf8(value)
    }
}
