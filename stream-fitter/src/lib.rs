//! Rusty library for linking and interfacing with chat streams.
pub mod errors;

/// Lifted error type used throughout this crate.
pub type Error = errors::FitterError;
/// Lifted result type used throughout this crate.
pub type Result<T> = errors::FitterResult<T>;
