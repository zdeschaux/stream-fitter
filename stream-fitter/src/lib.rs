//! Rusty library for linking and interfacing with chat streams.
pub mod clients;
pub mod errors;
pub mod pipe_fitter;

/// Lifted error type used throughout this crate.
pub type Error = errors::FitterError;
/// Lifted result type used throughout this crate.
pub type Result<T> = errors::FitterResult<T>;
