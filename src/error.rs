use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Format(&'static str),
    Unsupported(&'static str),
    Truncated,
    Zstd(String),
    Integrity(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Format(m) => write!(f, "invalid ARW: {m}"),
            Error::Unsupported(m) => write!(f, "unsupported ARW: {m}"),
            Error::Truncated => write!(f, "truncated file"),
            Error::Zstd(m) => write!(f, "zstd: {m}"),
            Error::Integrity(m) => write!(f, "integrity check failed: {m}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
