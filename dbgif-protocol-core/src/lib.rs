#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "std")]
extern crate alloc;

pub mod commands;
pub mod error;
pub mod header;
pub mod decoder;
pub mod encoder;
pub mod state;

#[cfg(feature = "ffi")]
pub mod ffi;

pub(crate) mod crc;

pub use commands::*;
pub use error::*;
pub use header::*;
pub use decoder::*;
pub use encoder::*;
pub use state::*;