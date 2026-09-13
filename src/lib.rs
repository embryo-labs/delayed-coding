//! A standalone implementation of Blitzcrank's Delayed Coding entropy algorithm.
//!
//! Models are immutable and shareable. Encoders use a forward scheduling pass
//! followed by backward embedding; decoders read symbols forward and may select
//! a different model for each symbol. Payloads are **raw**: the caller supplies
//! the same model, delay configuration and symbol count at decode time.
//!
//! ```
//! use delayed_coding::{Model, encode, decode_into};
//! let model = Model::from_counts(&[10, 5, 1])?;
//! let symbols = [0, 0, 1, 0, 2, 0];
//! let payload = encode::<24>(&model, &symbols)?;
//! let mut restored = [0; 6];
//! decode_into::<24>(&model, &payload, &mut restored)?;
//! assert_eq!(restored, symbols);
//! # Ok::<(), delayed_coding::Error>(())
//! ```
#![forbid(unsafe_code)]

mod branch;
mod codec;
mod layout;
mod model;

pub use branch::{Branch, Interval};
pub use codec::decode_grouped4_into;
pub use codec::decode_lookahead_interleaved_into;
pub use codec::encode_branches_into;
pub use codec::{decode_interleaved_into, encode_events_interleaved_into, encode_interleaved_into};
pub use codec::{
    decode_into, decode_lookahead_into, encode, encode_events_into, encode_into, max_encoded_size,
    Decoder, Event, Workspace,
};
pub use layout::Layout;
pub use model::{DecodedSymbol, Model, TableOptions, PROBABILITY_TOTAL};

/// All recoverable errors. Allocation failure follows Rust's allocator policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    InvalidModel,
    InvalidSymbol,
    InvalidDelay,
    InvalidLanes,
    InputTooLarge,
    OutputTooSmall,
    TruncatedInput,
    TrailingInput,
    InvalidState,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidModel => "expected 1..65536 frequencies summing to 65536",
            Self::InvalidSymbol => "symbol is absent from the model",
            Self::InvalidDelay => "delay must be between 16 and 32 bits",
            Self::InvalidLanes => "interleaving needs 1, 2, 4 or 8 states",
            Self::InputTooLarge => "input size exceeds addressable output capacity",
            Self::OutputTooSmall => "output buffer is too small",
            Self::TruncatedInput => "truncated entropy payload",
            Self::TrailingInput => "trailing bytes after entropy payload",
            Self::InvalidState => "invalid final coding state",
        })
    }
}

impl std::error::Error for Error {}
