//! Error utilities used throughout this crate.
use failure::Fail;

/// Error type used throughout this crate.
pub type FitterError = ::failure::Error;
/// Result type used throughout this crate.
pub type FitterResult<T> = ::std::result::Result<T, FitterError>;

/// Error kidn enum used throughout this crate.
#[derive(Fail, Debug)]
pub enum FitterErrorKind {
    #[fail(display = "Internal error: {}", _0)]
    InternalErr(String),
    #[fail(display = "Generic error: {}", _0)]
    GenericErr(String),
}
