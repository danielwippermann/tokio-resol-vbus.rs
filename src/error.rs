use std::{error::Error as StdError, fmt};

/// A common error type.
#[derive(Debug, PartialEq)]
pub struct Error {
    description: String,
}

impl Error {
    /// Construct a new `Error` using the provided description.
    pub fn new<T: fmt::Display>(description: T) -> Error {
        Error {
            description: format!("{}", description),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.description)
    }
}

impl StdError for Error {}

macro_rules! from_other_error {
    ($type:path) => {
        impl From<$type> for Error {
            fn from(cause: $type) -> Error {
                Error::new(cause)
            }
        }
    };
}

from_other_error!(std::io::Error);
from_other_error!(std::net::AddrParseError);
from_other_error!(std::str::Utf8Error);
from_other_error!(resol_vbus::Error);
from_other_error!(tokio::timer::Error);

/// A common result type.
pub type Result<T> = std::result::Result<T, Error>;
