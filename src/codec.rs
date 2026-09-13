use crate::{model::Symbol, Error, Layout, Model};
use std::ops::Range;

/// Models are borrowed for the duration of encoding; lifetimes prevent stale references.
#[derive(Clone, Copy)]
pub struct Event<'a> {
    pub model: &'a Model,
    pub symbol: u32,
}

/// Reuse between calls to avoid reallocating the per-symbol virtual schedule.
#[derive(Default)]
pub struct Workspace {
    virtual_symbols: Vec<u8>,
}
impl Workspace {
    pub fn with_capacity(symbols: usize) -> Self {
        Self {
            virtual_symbols: Vec::with_capacity(symbols),
        }
    }
    pub fn capacity(&self) -> usize {
        self.virtual_symbols.capacity()
    }
}

pub fn max_encoded_size(symbols: usize) -> Result<usize, Error> {
    symbols.checked_mul(2).ok_or(Error::InputTooLarge)
}

#[inline]
fn validate_delay<const DELAY: u32>() -> Result<(), Error> {
    if !(16..=32).contains(&DELAY) {
        Err(Error::InvalidDelay)
    } else {
        Ok(())
    }
}

// For valid input with DELAY <= 32, n < 2^48 and f <= 2^16. For f>=2,
// ceil(2^64/f) produces floor(n/f) exactly: its error is < n/2^64 < 1/f.
// Frequency 1 is handled separately. See docs/ALGORITHM.md for invariants.
#[inline]
pub(crate) fn quotient(n: u64, symbol: &Symbol) -> u64 {
    #[cfg(feature = "reference-division")]
    {
        n / u64::from(symbol.frequency)
    }
    #[cfg(not(feature = "reference-division"))]
    {
        if symbol.frequency == 1 {
            n
        } else {
            ((u128::from(n) * u128::from(symbol.reciprocal)) >> 64) as u64
        }
    }
}

fn encode_impl<'a, const DELAY: u32, const LANES: usize>(
    count: usize,
    event_at: impl Fn(usize) -> Event<'a>,
    output: &mut [u8],
    workspace: &mut Workspace,
) -> Result<Range<usize>, Error> {
    let bytes = schedule_impl::<DELAY, LANES>(count, &event_at, workspace)?;
    if bytes > output.len() {
        return Err(Error::OutputTooSmall);
    }
    Ok(embed_impl::<LANES>(count, &event_at, output, workspace))
}

fn schedule_impl<'a, const DELAY: u32, const LANES: usize>(
    count: usize,
    event_at: &impl Fn(usize) -> Event<'a>,
    workspace: &mut Workspace,
) -> Result<usize, Error> {
    let mut layout = Layout::<DELAY, LANES>::new()?;
    max_encoded_size(count)?;
    workspace.virtual_symbols.resize(count, 0);
    for i in 0..count {
        let event = event_at(i);
        let symbol = event
            .model
            .symbols
            .get(event.symbol as usize)
            .ok_or(Error::InvalidSymbol)?;
        if symbol.frequency == 0 {
            return Err(Error::InvalidSymbol);
        }
        workspace.virtual_symbols[i] = u8::from(layout.push_frequency(symbol.frequency)?.is_none());
    }
    Ok(layout.encoded_len())
}

fn embed_impl<'a, const LANES: usize>(
    count: usize,
    event_at: &impl Fn(usize) -> Event<'a>,
    output: &mut [u8],
    workspace: &Workspace,
) -> Range<usize> {
    let mut position = output.len();
    let mut numerators = [0u64; LANES];
    for i in (0..count).rev() {
        let numerator = &mut numerators[i & (LANES - 1)];
        let event = event_at(i);
        let symbol = &event.model.symbols[event.symbol as usize];
        debug_assert!(*numerator < (1u64 << 48));
        let q = quotient(*numerator, symbol);
        let remainder = (*numerator - q * u64::from(symbol.frequency)) as u32;
        let word = event.model.embed_validated(symbol, remainder);
        *numerator = q;
        if workspace.virtual_symbols[i] != 0 {
            *numerator = (*numerator << 16) | u64::from(word);
        } else {
            position -= 2;
            output[position..position + 2].copy_from_slice(&word.to_be_bytes());
        }
    }
    position..output.len()
}

/// Writes backwards, returning the payload's range in the caller's buffer.
/// Input validity and capacity are checked before any payload byte is written.
pub fn encode_into<const DELAY: u32>(
    model: &Model,
    symbols: &[u32],
    output: &mut [u8],
    workspace: &mut Workspace,
) -> Result<Range<usize>, Error> {
    encode_impl::<DELAY, 1>(
        symbols.len(),
        |i| Event {
            model,
            symbol: symbols[i],
        },
        output,
        workspace,
    )
}

/// Encode a sequence whose probability model may change at every symbol.
pub fn encode_events_into<const DELAY: u32>(
    events: &[Event<'_>],
    output: &mut [u8],
    workspace: &mut Workspace,
) -> Result<Range<usize>, Error> {
    encode_impl::<DELAY, 1>(events.len(), |i| events[i], output, workspace)
}

/// Round-robin interleaving of 1, 2, 4 or 8 independent coding states into one
/// payload. The lane count is external format metadata. No per-lane length header
/// is needed: encoder and decoder use the same deterministic physical-word order.
pub fn encode_interleaved_into<const DELAY: u32, const LANES: usize>(
    model: &Model,
    symbols: &[u32],
    output: &mut [u8],
    workspace: &mut Workspace,
) -> Result<Range<usize>, Error> {
    encode_impl::<DELAY, LANES>(
        symbols.len(),
        |i| Event {
            model,
            symbol: symbols[i],
        },
        output,
        workspace,
    )
}

/// Interleaved encoding with explicit per-symbol models. The caller remains
/// responsible for selecting contexts in forward order before encoding.
pub fn encode_events_interleaved_into<const DELAY: u32, const LANES: usize>(
    events: &[Event<'_>],
    output: &mut [u8],
    workspace: &mut Workspace,
) -> Result<Range<usize>, Error> {
    encode_impl::<DELAY, LANES>(events.len(), |i| events[i], output, workspace)
}

/// Allocates exactly the payload length, without a worst-case payload buffer or
/// a final payload move. The per-symbol scheduling workspace is still required.
/// Use encode_into and a reusable workspace in hot paths.
pub fn encode<const DELAY: u32>(model: &Model, symbols: &[u32]) -> Result<Vec<u8>, Error> {
    let event_at = |i| Event {
        model,
        symbol: symbols[i],
    };
    let mut workspace = Workspace::default();
    let bytes = schedule_impl::<DELAY, 1>(symbols.len(), &event_at, &mut workspace)?;
    let mut output = vec![0; bytes];
    let range = embed_impl::<1>(symbols.len(), &event_at, &mut output, &workspace);
    debug_assert_eq!(range.start, 0);
    Ok(output)
}

/// Forward decoder with explicit model selection. All input reads are bounded.
/// Errors are sticky; discard the decoder after a failed read. Call finish after
/// decoding the externally supplied symbol count. This is not a checksum.
pub struct Decoder<'a, const DELAY: u32 = 24, const LANES: usize = 1> {
    input: &'a [u8],
    position: usize,
    states: [CodingState; LANES],
    lane: usize,
    error: Option<Error>,
}

#[derive(Clone, Copy)]
struct CodingState {
    numerator: u64,
    denominator: u64,
}

impl<'a, const DELAY: u32, const LANES: usize> Decoder<'a, DELAY, LANES> {
    pub fn new(input: &'a [u8]) -> Result<Self, Error> {
        validate_delay::<DELAY>()?;
        if !matches!(LANES, 1 | 2 | 4 | 8) {
            return Err(Error::InvalidLanes);
        }
        Ok(Self {
            input,
            position: 0,
            states: [CodingState {
                numerator: 0,
                denominator: 1,
            }; LANES],
            lane: 0,
            error: None,
        })
    }

    #[inline]
    pub fn read(&mut self, model: &Model) -> Result<u32, Error> {
        if let Some(error) = self.error {
            return Err(error);
        }
        let state = &mut self.states[self.lane];
        // Renormalize when a word is consumed, instead of materializing an
        // Option after every symbol and checking it again on the next read.
        // The denominator already carries the information that a virtual word
        // exists; the numerator's low bits are that word.
        let word = if state.denominator >= (1u64 << DELAY) {
            let word = state.numerator as u16;
            state.numerator >>= 16;
            state.denominator >>= 16;
            word
        } else {
            let Some(bytes) = self
                .input
                .get(self.position..self.position.saturating_add(2))
            else {
                self.error = Some(Error::TruncatedInput);
                return Err(Error::TruncatedInput);
            };
            self.position += 2;
            u16::from_be_bytes([bytes[0], bytes[1]])
        };
        let decoded = model.lookup(word);
        // Malformed payloads must not panic on integer overflow in Debug builds.
        // The valid-stream invariant is stronger; wrapping arithmetic here does
        // not make malformed streams trusted or guarantee corruption detection.
        state.numerator = state
            .numerator
            .wrapping_mul(u64::from(decoded.frequency))
            .wrapping_add(u64::from(decoded.remainder));
        state.denominator *= u64::from(decoded.frequency);
        self.lane = (self.lane + 1) & (LANES - 1);
        Ok(decoded.symbol)
    }

    pub fn bytes_read(&self) -> usize {
        self.position
    }
    pub fn finish(&self) -> Result<(), Error> {
        if let Some(error) = self.error {
            return Err(error);
        }
        if self.position != self.input.len() {
            return Err(Error::TrailingInput);
        }
        if self.states.iter().any(|s| s.numerator != 0) {
            return Err(Error::InvalidState);
        }
        Ok(())
    }
}

/// Decode exactly output.len() symbols, then check consumption and final state.
/// On error, output may contain a decoded prefix.
pub fn decode_into<const DELAY: u32>(
    model: &Model,
    input: &[u8],
    output: &mut [u32],
) -> Result<(), Error> {
    decode_interleaved_into::<DELAY, 1>(model, input, output)
}

/// Experimental single-state, fixed-model decoder with one physical word of lookahead.
///
/// Uses the exact same payload as `decode_into`. The next physical word's model
/// lookup is independent of the current information-state update. Keeping its
/// decoded entry in registers can overlap those operations. No allocation or
/// additional model table is required. This is not suitable for per-symbol model
/// selection, and is not necessarily faster on every distribution or processor.
/// All reads are bounded; final validation is identical to `decode_into`.
pub fn decode_lookahead_into<const DELAY: u32>(
    model: &Model,
    input: &[u8],
    output: &mut [u32],
) -> Result<(), Error> {
    validate_delay::<DELAY>()?;
    let mut physical = input
        .chunks_exact(2)
        .map(|bytes| model.lookup(u16::from_be_bytes([bytes[0], bytes[1]])));
    let mut pending = physical.next();
    let (mut position, mut numerator, mut denominator) = (0usize, 0u64, 1u64);
    for symbol in output {
        let decoded = if denominator >= 1u64 << DELAY {
            let decoded = model.lookup(numerator as u16);
            numerator >>= 16;
            denominator >>= 16;
            decoded
        } else {
            let decoded = pending.ok_or(Error::TruncatedInput)?;
            pending = physical.next();
            position += 2;
            decoded
        };
        numerator = numerator
            .wrapping_mul(u64::from(decoded.frequency))
            .wrapping_add(u64::from(decoded.remainder));
        denominator *= u64::from(decoded.frequency);
        *symbol = decoded.symbol;
    }
    if position != input.len() {
        return Err(Error::TrailingInput);
    }
    if numerator != 0 {
        return Err(Error::InvalidState);
    }
    Ok(())
}

pub fn decode_interleaved_into<const DELAY: u32, const LANES: usize>(
    model: &Model,
    input: &[u8],
    output: &mut [u32],
) -> Result<(), Error> {
    let mut decoder = Decoder::<DELAY, LANES>::new(input)?;
    for symbol in output {
        *symbol = decoder.read(model)?;
    }
    decoder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reciprocals_match_division_at_state_boundaries() {
        for frequency in 1..=65536u32 {
            let model = Model::new(&[frequency, 65536 - frequency]).unwrap();
            let symbol = &model.symbols[0];
            let maximum = (1u64 << 48) - 1;
            let near_multiple = (maximum / u64::from(frequency) - 1) * u64::from(frequency);
            for n in [
                0,
                1,
                u64::from(frequency) - 1,
                u64::from(frequency),
                near_multiple - 1,
                near_multiple,
                near_multiple + 1,
                maximum,
            ] {
                assert_eq!(
                    quotient(n, symbol),
                    n / u64::from(frequency),
                    "f={frequency}, n={n}"
                );
            }
        }
    }
}
