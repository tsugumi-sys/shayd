use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    InvalidDatabaseHeader(&'static str),
    InvalidPageSize(u16),
    InvalidTextEncoding(u32),
    InvalidVarint,
    ReservedSerialType(u64),
    Truncated {
        context: &'static str,
        needed: usize,
        available: usize,
    },
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
            Self::InvalidDatabaseHeader(message) => {
                write!(f, "invalid database header: {message}")
            }
            Self::InvalidPageSize(size) => write!(f, "invalid database page size: {size}"),
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
