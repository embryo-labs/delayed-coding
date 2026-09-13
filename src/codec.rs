use crate::{model::Symbol, Branch, Error, Layout, Model};
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
    quotient_frequency(n, symbol.frequency, symbol.reciprocal)
}

#[inline]
pub(crate) fn quotient_frequency(n: u64, frequency: u32, reciprocal: u64) -> u64 {
    #[cfg(feature = "reference-division")]
    {
        let _ = reciprocal;
        n / u64::from(frequency)
    }
    #[cfg(not(feature = "reference-division"))]
    {
        if frequency == 1 {
            n
        } else {
            ((u128::from(n) * u128::from(reciprocal)) >> 64) as u64
        }
    }
}

/// Encode an existing sequence of selected semantic-model branches. Unlike
/// model-symbol events this preserves arbitrary disjoint interval mappings,
/// including contiguous numerical/rare-value partitions and raw words.
///
/// The iterator must yield the same branches when cloned. A reusable workspace
/// avoids per-block allocation; an iterator also lets the C ABI borrow its array
/// of handles without allocating a temporary array of Rust references.
/// Only 1/2/4/8 round-robin lanes and delay 16..32 are supported.
pub fn encode_branches_into<'a, const DELAY: u32, const LANES: usize>(
    branches: impl DoubleEndedIterator<Item = &'a Branch> + ExactSizeIterator + Clone,
    output: &mut [u8],
    workspace: &mut Workspace,
) -> Result<Range<usize>, Error> {
    let mut layout = Layout::<DELAY, LANES>::new()?;
    let count = branches.len();
    max_encoded_size(count)?;
    workspace.virtual_symbols.resize(count, 0);
    let mut seen = 0;
    for branch in branches.clone() {
        let flag = workspace
            .virtual_symbols
            .get_mut(seen)
            .ok_or(Error::InvalidState)?;
        *flag = u8::from(layout.push_validated(branch.frequency()).is_none());
        seen += 1;
    }
    if seen != count {
        return Err(Error::InvalidState);
    }
    if layout.encoded_len() > output.len() {
        return Err(Error::OutputTooSmall);
    }
    let mut position = output.len();
    let mut numerators = [0u64; LANES];
    for branch in branches.rev() {
        seen = seen.checked_sub(1).ok_or(Error::InvalidState)?;
        let numerator = &mut numerators[seen & (LANES - 1)];
        // Guard even an inconsistent user-supplied iterator in reference-division mode.
        if *numerator >= 1u64 << 48 {
            return Err(Error::InvalidState);
        }
        let (quotient, word) = branch.split(*numerator);
        *numerator = quotient;
        if workspace.virtual_symbols[seen] != 0 {
            *numerator = (*numerator << 16) | u64::from(word);
        } else {
            position = position.checked_sub(2).ok_or(Error::OutputTooSmall)?;
            output[position..position + 2].copy_from_slice(&word.to_be_bytes());
        }
    }
    if seen != 0 {
        return Err(Error::InvalidState);
    }
    Ok(position..output.len())
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
        workspace.virtual_symbols[i] = u8::from(layout.push_validated(symbol.frequency).is_none());
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
        if cfg!(feature = "speculative-encode") && LANES >= 4 {
            let virtual_word = usize::from(workspace.virtual_symbols[i]);
            // Every lane starts with a physical word. Thus while reversing, every
            // virtual word still has an earlier physical slot to overwrite. This
            // store stays inside the final occupied range even for exact capacity.
            output[position - 2..position].copy_from_slice(&word.to_be_bytes());
            position -= (1 - virtual_word) * 2;
            *numerator = (q << (virtual_word * 16))
                | (u64::from(word) & (virtual_word as u64).wrapping_neg());
        } else {
            *numerator = q;
            if workspace.virtual_symbols[i] != 0 {
                *numerator = (*numerator << 16) | u64::from(word);
            } else {
                position -= 2;
                output[position..position + 2].copy_from_slice(&word.to_be_bytes());
            }
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

    #[inline(always)]
    pub fn read(&mut self, model: &Model) -> Result<u32, Error> {
        self.read_mapped(|word| Ok(model.lookup(word)))
    }

    /// Read one of `count` contiguous equal-width intervals, preserving the
    /// original numerical/rare-branch mapping. Rejects unused codeword tails.
    #[inline]
    pub fn read_uniform(&mut self, frequency: u32, count: u32) -> Result<u32, Error> {
        if let Some(error) = self.error {
            return Err(error);
        }
        if frequency == 0 || frequency > 65536 || count == 0 || count > 65536 / frequency {
            self.error = Some(Error::InvalidModel);
            return Err(Error::InvalidModel);
        }
        self.read_mapped(|word| {
            let word = u32::from(word);
            let symbol = word / frequency;
            if symbol >= count {
                return Err(Error::InvalidState);
            }
            Ok(crate::DecodedSymbol {
                symbol,
                frequency,
                remainder: word - symbol * frequency,
            })
        })
    }

    /// A frequency-one operation; consumes a virtual word when present, without
    /// allocating a 65536-symbol identity model. Participates in lane assignment.
    #[inline]
    pub fn read_raw(&mut self) -> Result<u16, Error> {
        self.read_uniform(1, 65536).map(|word| word as u16)
    }

    #[inline(always)]
    fn read_mapped(
        &mut self,
        lookup: impl FnOnce(u16) -> Result<crate::DecodedSymbol, Error>,
    ) -> Result<u32, Error> {
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
        let decoded = match lookup(word) {
            Ok(decoded) => decoded,
            Err(error) => {
                self.error = Some(error);
                return Err(error);
            }
        };
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
    decode_lookahead_interleaved_into::<DELAY, 1>(model, input, output)
}

/// The fixed-model physical lookahead path with 1/2/4/8 independent states.
/// One pending physical lookup is shared across lanes because the model is
/// fixed; no assumptions are made about which lane consumes the next word.
/// Uses the same bitstream as `decode_interleaved_into` for the same lane count.
#[inline(never)]
pub fn decode_lookahead_interleaved_into<const DELAY: u32, const LANES: usize>(
    model: &Model,
    input: &[u8],
    output: &mut [u32],
) -> Result<(), Error> {
    validate_delay::<DELAY>()?;
    if !matches!(LANES, 1 | 2 | 4 | 8) {
        return Err(Error::InvalidLanes);
    }
    let mut physical = input
        .chunks_exact(2)
        .map(|bytes| model.lookup(u16::from_be_bytes([bytes[0], bytes[1]])));
    let mut pending = physical.next();
    let mut position = 0usize;
    let mut states = [CodingState {
        numerator: 0,
        denominator: 1,
    }; LANES];
    for (i, symbol) in output.iter_mut().enumerate() {
        let state = &mut states[i & (LANES - 1)];
        let decoded = if state.denominator >= 1u64 << DELAY {
            let decoded = model.lookup(state.numerator as u16);
            state.numerator >>= 16;
            state.denominator >>= 16;
            decoded
        } else {
            let decoded = pending.ok_or(Error::TruncatedInput)?;
            pending = physical.next();
            position += 2;
            decoded
        };
        state.numerator = state
            .numerator
            .wrapping_mul(u64::from(decoded.frequency))
            .wrapping_add(u64::from(decoded.remainder));
        state.denominator *= u64::from(decoded.frequency);
        *symbol = decoded.symbol;
    }
    if position != input.len() {
        return Err(Error::TrailingInput);
    }
    if states.iter().any(|state| state.numerator != 0) {
        return Err(Error::InvalidState);
    }
    Ok(())
}

// Isolate block-kernel optimization from growing C ABI dispatch switches. Inner
// symbol operations remain inline; this boundary is crossed once per block.
#[inline(never)]
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

/// Experimental four-state kernel: plan a group's physical offsets from all
/// four capacities before any symbol lookup. Same four-lane fixed-model format.
/// This is an explicit alternative, not the default decoder.
#[inline(never)]
pub fn decode_grouped4_into<const DELAY: u32>(
    model: &Model,
    input: &[u8],
    output: &mut [u32],
) -> Result<(), Error> {
    grouped4_impl::<DELAY, false>(model, input, output)
}

/// Experimental branchless four-state source selection. A bounded eight-byte
/// window permits speculative word loads inside the payload, never outside it.
/// No caller padding is needed; scalar bounded reads handle the entire tail.
#[inline(never)]
pub fn decode_branchless4_into<const DELAY: u32>(
    model: &Model,
    input: &[u8],
    output: &mut [u32],
) -> Result<(), Error> {
    grouped4_impl::<DELAY, true>(model, input, output)
}

fn grouped4_impl<const DELAY: u32, const BRANCHLESS: bool>(
    model: &Model,
    input: &[u8],
    output: &mut [u32],
) -> Result<(), Error> {
    if let Some(table) = model.direct_decode_table() {
        grouped4_lookup::<DELAY, BRANCHLESS>(model, input, output, |word| {
            let entry = table[usize::from(word)];
            crate::DecodedSymbol {
                symbol: ((entry >> 16) & 65535) as u32,
                frequency: (entry >> 32) as u32 + 1,
                remainder: (entry & 65535) as u32,
            }
        })
    } else {
        grouped4_lookup::<DELAY, BRANCHLESS>(model, input, output, |word| model.lookup(word))
    }
}

#[inline(always)]
fn grouped4_lookup<const DELAY: u32, const BRANCHLESS: bool>(
    model: &Model,
    input: &[u8],
    output: &mut [u32],
    lookup: impl Fn(u16) -> crate::DecodedSymbol,
) -> Result<(), Error> {
    let mut decoder = Decoder::<DELAY, 4>::new(input)?;
    let mut completed = 0;
    while completed + 4 <= output.len() {
        if BRANCHLESS && input.len() - decoder.position < 8 {
            break;
        }
        let out = &mut output[completed..completed + 4];
        let physical = decoder
            .states
            .map(|s| usize::from(s.denominator < (1u64 << DELAY)));
        let offsets = [
            0,
            physical[0],
            physical[0] + physical[1],
            physical[0] + physical[1] + physical[2],
        ];
        let count = offsets[3] + physical[3];
        let bytes = if BRANCHLESS { 8 } else { count * 2 };
        let Some(window) = input.get(decoder.position..decoder.position.saturating_add(bytes))
        else {
            break;
        };
        let mut words = [0u16; 4];
        for lane in 0..4 {
            let state = &mut decoder.states[lane];
            words[lane] = if BRANCHLESS {
                let loaded =
                    u16::from_be_bytes([window[2 * offsets[lane]], window[2 * offsets[lane] + 1]]);
                let mask = (physical[lane] as u16).wrapping_neg();
                let word = (loaded & mask) | (state.numerator as u16 & !mask);
                let shift = (1 - physical[lane]) * 16;
                state.numerator >>= shift;
                state.denominator >>= shift;
                word
            } else if physical[lane] != 0 {
                u16::from_be_bytes([window[2 * offsets[lane]], window[2 * offsets[lane] + 1]])
            } else {
                let word = state.numerator as u16;
                state.numerator >>= 16;
                state.denominator >>= 16;
                word
            };
        }
        decoder.position += count * 2;
        let decoded = words.map(&lookup);
        for lane in 0..4 {
            let state = &mut decoder.states[lane];
            state.numerator = state
                .numerator
                .wrapping_mul(u64::from(decoded[lane].frequency))
                .wrapping_add(u64::from(decoded[lane].remainder));
            state.denominator *= u64::from(decoded[lane].frequency);
            out[lane] = decoded[lane].symbol;
        }
        completed += 4;
    }
    for symbol in &mut output[completed..] {
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
