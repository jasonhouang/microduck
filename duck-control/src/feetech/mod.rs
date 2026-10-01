//! Feetech HD1910 (FT-SCS protocol) driver, adapted from the standalone tool.

pub mod error;
pub mod protocol;
pub mod registers;
pub mod servo;

pub use error::{Result, ServoError};
