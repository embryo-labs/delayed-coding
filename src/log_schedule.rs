//! Experimental conservative logarithmic schedule; a separate raw format.
use crate::{Error, Workspace};
use std::ops::Range;

const THRESHOLD: u32 = 24 * 256;
const RENORM: u32 = 16 * 256 + 2;

#[cfg(test)]
#[path = "log_schedule_tests.rs"]
mod tests;

#[derive(Clone, Copy)]
struct Symbol {
    frequency: u32,
    start: u32,
    reciprocal: u64,
}

/// Experimental DC derivative with cumulative intervals and an integer log
/// schedule. Not compatible with the ordinary Delayed Coding payload format.
pub struct LogModel {
    symbols: Vec<Symbol>,
    logs: Vec<u16>,
    table: Option<Box<[u64; 65536]>>,
    lookup8: Option<Box<[u8; 65536]>>,
    decode_symbols: Vec<u64>,
}

// Lower bound on 256*log2(f), computed using only integer arithmetic.
fn log_frequency(f: u32) -> u32 {
    let integer = 31 - f.leading_zeros();
    let mut normalized = u64::from(f) << (31 - integer);
    let mut fractional = 0;
    for _ in 0..8 {
        normalized = (normalized * normalized) >> 31;
        fractional <<= 1;
        if normalized >= 1u64 << 32 {
            normalized >>= 1;
            fractional |= 1;
        }
    }
    integer * 256 + fractional
}

impl LogModel {
    /// Prepare a fixed model with 1..=65536 symbol IDs and frequencies summing
    /// to 65536. Zero-frequency IDs remain invalid, including padded byte IDs.
    pub fn new(frequencies: &[u32]) -> Result<Self, Error> {
        if frequencies.is_empty()
            || frequencies.len() > 65536
            || frequencies.iter().map(|&f| u64::from(f)).sum::<u64>() != 65536
        {
            return Err(Error::InvalidModel);
        }
        let mut symbols = Vec::with_capacity(frequencies.len());
        let mut logs = Vec::with_capacity(frequencies.len());
        let mut lookup8 = (frequencies.len() <= 256).then(|| vec![0u8; 65536]);
        let mut decode_symbols = Vec::with_capacity(frequencies.len());
        let mut table = (frequencies.len() > 256).then(|| vec![0u64; 65536]);
        let mut start = 0;
        for (id, &frequency) in frequencies.iter().enumerate() {
            let log = if frequency == 0 {
                0
            } else {
                log_frequency(frequency)
            };
            logs.push(if frequency == 0 { u16::MAX } else { log as u16 });
            decode_symbols.push(
                u64::from(frequency) | (u64::from(start as u16) << 32) | (u64::from(log) << 48),
            );
            symbols.push(Symbol {
                frequency,
                start,
                reciprocal: if frequency > 1 {
                    u64::MAX / u64::from(frequency) + 1
                } else {
                    0
                },
            });
            for remainder in 0..frequency {
                if let Some(lookup) = &mut lookup8 {
                    lookup[(start + remainder) as usize] = id as u8;
                }
                if let Some(table) = &mut table {
                    table[(start + remainder) as usize] = u64::from(remainder)
                        | ((id as u64) << 16)
                        | (u64::from(frequency - 1) << 32)
                        | (u64::from(log) << 48);
                }
            }
            start += frequency;
        }
        if symbols.len() < 256 {
            symbols.resize(
                256,
                Symbol {
                    frequency: 0,
                    start: 0,
                    reciprocal: 0,
                },
            );
            logs.resize(256, u16::MAX);
            decode_symbols.resize(256, 0);
        }
        Ok(Self {
            symbols,
            logs,
            table: table.map(|v| v.into_boxed_slice().try_into().unwrap()),
            lookup8: lookup8.map(|v| v.into_boxed_slice().try_into().unwrap()),
            decode_symbols,
        })
    }

    /// Allocated model table bytes, excluding headers and allocator bookkeeping.
    pub fn table_bytes(&self) -> usize {
        self.symbols.capacity() * std::mem::size_of::<Symbol>()
            + self.logs.capacity() * 2
            + self.decode_symbols.capacity() * 8
            + self.lookup8.as_ref().map_or(0, |_| 65536)
            + self.table.as_ref().map_or(0, |_| 65536 * 8)
    }

    /// Encode backwards into a reusable buffer, returning its occupied range.
    /// 2*input.len() bytes suffice. Validation and exact capacity checking happen
    /// before any output write; Workspace reuses one schedule byte per symbol.
    /// Lane count (1/2/4/8), model and symbol count are external format metadata.
    pub fn encode_into<const LANES: usize>(
        &self,
        input: &[u32],
        output: &mut [u8],
        workspace: &mut Workspace,
    ) -> Result<Range<usize>, Error> {
        let schedule = &mut workspace.virtual_symbols;
        if let (Ok(symbols), Ok(logs)) = (
            <&[Symbol; 256]>::try_from(self.symbols.as_slice()),
            <&[u16; 256]>::try_from(self.logs.as_slice()),
        ) {
            self.encode_impl::<LANES>(
                input,
                output,
                schedule,
                |id| symbols[usize::from(id as u8)],
                |id| (logs[usize::from(id as u8)], id >> 8),
            )
        } else {
            self.encode_impl::<LANES>(
                input,
                output,
                schedule,
                |id| self.symbols[id as usize],
                |id| (self.logs.get(id as usize).copied().unwrap_or(u16::MAX), 0),
            )
        }
    }

    #[inline(always)]
    #[cfg(test)]
    fn encode_reverse_impl<const LANES: usize>(
        &self,
        input: &[u32],
        output: &mut [u8],
        _schedule: &mut Vec<u8>,
        symbol_at: impl Fn(u32) -> Symbol,
        log_at: impl Fn(u32) -> (u16, u32),
    ) -> Result<Range<usize>, Error> {
        if !matches!(LANES, 1 | 2 | 4 | 8) {
            return Err(Error::InvalidLanes);
        }
        input.len().checked_mul(2).ok_or(Error::InputTooLarge)?;
        if input.len() as u64 > u64::MAX / 4096 {
            return Err(Error::InputTooLarge);
        }
        let mut sums = [0u64; LANES];
        for ids in input.as_chunks::<LANES>().0 {
            let mut logs = [0u64; LANES];
            let mut invalid = 0;
            for i in 0..LANES {
                let (log, high) = log_at(ids[i]);
                invalid |= high | u32::from(log == u16::MAX);
                logs[i] = u64::from(log);
            }
            if invalid != 0 {
                return Err(Error::InvalidSymbol);
            }
            for i in 0..LANES {
                sums[i] += logs[i];
            }
        }
        for (i, &id) in input.as_chunks::<LANES>().1.iter().enumerate() {
            let (log, high) = log_at(id);
            if log == u16::MAX || high != 0 {
                return Err(Error::InvalidSymbol);
            }
            sums[i] += u64::from(log);
        }
        let mut first_virtual = [usize::MAX; LANES];
        let mut capacities = [0i32; LANES];
        let mut virtual_count = 0usize;
        const BASE: u64 = (THRESHOLD - RENORM) as u64;
        for lane in 0..LANES {
            if lane >= input.len() {
                continue;
            }
            let last = lane + (input.len() - 1 - lane) / LANES * LANES;
            let sum = sums[lane] - u64::from(log_at(input[last]).0);
            let reductions = if sum < u64::from(THRESHOLD) {
                0
            } else {
                (sum - BASE) / u64::from(RENORM)
            };
            virtual_count += reductions as usize;
            capacities[lane] = (sum - reductions * u64::from(RENORM)) as i32;
            if reductions != 0 {
                let mut prefix = 0;
                let mut i = lane;
                while prefix < THRESHOLD {
                    prefix += u32::from(log_at(input[i]).0);
                    i += LANES;
                }
                first_virtual[lane] = i;
            }
        }
        let bytes = (input.len() - virtual_count) * 2;
        if bytes > output.len() {
            return Err(Error::OutputTooSmall);
        }
        let mut position = output.len();
        let mut states = [0u64; LANES];
        let embed = |i: usize,
                     state: &mut u64,
                     capacity: &mut i32,
                     first: usize,
                     position: &mut usize,
                     output: &mut [u8]| {
            let previous_log = if i >= LANES {
                log_at(input[i - LANES]).0
            } else {
                0
            };
            let previous = *capacity - i32::from(previous_log);
            let virtual_word = usize::from(i >= first && previous < BASE as i32);
            *capacity = previous + virtual_word as i32 * RENORM as i32;
            let symbol = symbol_at(input[i]);
            let q = *state / u64::from(symbol.frequency);
            let word = (*state - q * u64::from(symbol.frequency) + u64::from(symbol.start)) as u16;
            output[*position - 2..*position].copy_from_slice(&word.to_be_bytes());
            *position -= (1 - virtual_word) * 2;
            *state = (q << (virtual_word * 16))
                | (u64::from(word) & (virtual_word as u64).wrapping_neg());
        };
        let full = input.len() / LANES * LANES;
        for i in (full..input.len()).rev() {
            let lane = i % LANES;
            embed(
                i,
                &mut states[lane],
                &mut capacities[lane],
                first_virtual[lane],
                &mut position,
                output,
            );
        }
        for group in (0..full / LANES).rev() {
            for lane in (0..LANES).rev() {
                embed(
                    group * LANES + lane,
                    &mut states[lane],
                    &mut capacities[lane],
                    first_virtual[lane],
                    &mut position,
                    output,
                );
            }
        }
        debug_assert_eq!(position, output.len() - bytes);
        debug_assert_eq!(capacities, [0; LANES]);
        debug_assert_eq!(states, [0; LANES]);
        Ok(position..output.len())
    }

    #[inline(always)]
    fn encode_impl<const LANES: usize>(
        &self,
        input: &[u32],
        output: &mut [u8],
        schedule: &mut Vec<u8>,
        symbol_at: impl Fn(u32) -> Symbol,
        log_at: impl Fn(u32) -> (u16, u32),
    ) -> Result<Range<usize>, Error> {
        if !matches!(LANES, 1 | 2 | 4 | 8) {
            return Err(Error::InvalidLanes);
        }
        input.len().checked_mul(2).ok_or(Error::InputTooLarge)?;
        schedule.resize(input.len(), 0);
        let mut budgets = [0u32; LANES];
        let mut bytes = 0;
        let full = input.len() / LANES * LANES;
        for (ids, flags) in input
            .as_chunks::<LANES>()
            .0
            .iter()
            .zip(schedule.as_chunks_mut::<LANES>().0)
        {
            let mut logs = [0u32; LANES];
            let mut invalid = 0;
            for i in 0..LANES {
                let (log, high) = log_at(ids[i]);
                invalid |= high | u32::from(log == u16::MAX);
                logs[i] = u32::from(log);
            }
            if invalid != 0 {
                return Err(Error::InvalidSymbol);
            }
            for i in 0..LANES {
                let virtual_word = u32::from(budgets[i] >= THRESHOLD);
                flags[i] = virtual_word as u8;
                budgets[i] = budgets[i] + logs[i] - virtual_word * RENORM;
                bytes += (1 - virtual_word) as usize * 2;
            }
        }
        for i in 0..input.len() - full {
            let (log, high) = log_at(input[full + i]);
            if log == u16::MAX || high != 0 {
                return Err(Error::InvalidSymbol);
            }
            let virtual_word = u32::from(budgets[i] >= THRESHOLD);
            schedule[full + i] = virtual_word as u8;
            bytes += (1 - virtual_word) as usize * 2;
        }
        if bytes > output.len() {
            return Err(Error::OutputTooSmall);
        }
        let mut position = output.len();
        let mut states = [0u64; LANES];
        let embed = |id: u32, flag: u8, n: &mut u64, position: &mut usize, output: &mut [u8]| {
            let symbol = symbol_at(id);
            debug_assert!(*n < 1u64 << 48);
            let q = crate::codec::quotient_frequency(*n, symbol.frequency, symbol.reciprocal);
            let embedded = *n + q * u64::from(65536 - symbol.frequency) + u64::from(symbol.start);
            let word = embedded as u16;
            let virtual_word = usize::from(flag);
            output[*position - 2..*position].copy_from_slice(&word.to_be_bytes());
            *position -= (1 - virtual_word) * 2;
            *n = if virtual_word != 0 { embedded } else { q };
        };
        for i in (0..input.len() - full).rev() {
            embed(
                input[full + i],
                schedule[full + i],
                &mut states[i],
                &mut position,
                output,
            );
        }
        for (ids, flags) in input
            .as_chunks::<LANES>()
            .0
            .iter()
            .zip(schedule.as_chunks::<LANES>().0)
            .rev()
        {
            for i in (0..LANES).rev() {
                embed(ids[i], flags[i], &mut states[i], &mut position, output);
            }
        }
        debug_assert!(states.iter().all(|&n| n == 0));
        Ok(position..output.len())
    }

    /// Decode exactly output.len() symbols using bounded reads, then validate
    /// consumption and final zero states. Output can contain a prefix on error.
    /// No padding or per-call allocation is required. This is not a checksum.
    #[inline(never)]
    pub fn decode_into<const LANES: usize>(
        &self,
        input: &[u8],
        output: &mut [u32],
    ) -> Result<(), Error> {
        if let Some(lookup) = &self.lookup8 {
            let decode_symbols: &[u64; 256] = self
                .decode_symbols
                .as_slice()
                .try_into()
                .expect("byte alphabet table");
            self.decode_impl::<LANES>(input, output, |word| {
                let id = lookup[usize::from(word)];
                let symbol = u32::from(id);
                let entry = decode_symbols[usize::from(id)];
                (
                    u64::from(entry as u32),
                    u32::from(word) - u32::from((entry >> 32) as u16),
                    symbol,
                    (entry >> 48) as u32,
                )
            })
        } else {
            self.decode_impl::<LANES>(input, output, |word| {
                let entry = self.table.as_ref().expect("wide alphabet table")[usize::from(word)];
                (
                    ((entry >> 32) & 65535) + 1,
                    (entry & 65535) as u32,
                    ((entry >> 16) & 65535) as u32,
                    (entry >> 48) as u32,
                )
            })
        }
    }

    #[inline(always)]
    fn decode_impl<const LANES: usize>(
        &self,
        input: &[u8],
        output: &mut [u32],
        lookup: impl Fn(u16) -> (u64, u32, u32, u32),
    ) -> Result<(), Error> {
        if !matches!(LANES, 1 | 2 | 4 | 8) {
            return Err(Error::InvalidLanes);
        }
        let mut budgets = [0u32; LANES];
        let mut states = [0u64; LANES];
        let mut position = 0;
        let mut completed = 0;
        while completed + LANES <= output.len() && input.len() - position >= LANES * 2 {
            let out = &mut output[completed..completed + LANES];
            let window = &input[position..position + LANES * 2];
            let mut offsets = [0usize; LANES];
            let physical = budgets.map(|b| usize::from(b < THRESHOLD));
            let mut count = 0;
            for i in 0..LANES {
                offsets[i] = count;
                count += physical[i];
            }
            for i in 0..LANES {
                let loaded =
                    u16::from_be_bytes([window[offsets[i] * 2], window[offsets[i] * 2 + 1]]);
                let mask = (physical[i] as u16).wrapping_neg();
                let word = (loaded & mask) | (states[i] as u16 & !mask);
                states[i] = if physical[i] == 0 {
                    states[i] >> 16
                } else {
                    states[i]
                };
                let (frequency, remainder, symbol, log) = lookup(word);
                states[i] = states[i]
                    .wrapping_mul(frequency)
                    .wrapping_add(u64::from(remainder));
                budgets[i] = budgets[i] + log - (1 - physical[i]) as u32 * RENORM;
                out[i] = symbol;
            }
            position += count * 2;
            completed += LANES;
        }
        for (i, out) in output.iter_mut().enumerate().skip(completed) {
            let lane = i & (LANES - 1);
            let word = if budgets[lane] >= THRESHOLD {
                budgets[lane] -= RENORM;
                let word = states[lane] as u16;
                states[lane] >>= 16;
                word
            } else {
                let bytes = input
                    .get(position..position + 2)
                    .ok_or(Error::TruncatedInput)?;
                position += 2;
                u16::from_be_bytes([bytes[0], bytes[1]])
            };
            let (frequency, remainder, symbol, log) = lookup(word);
            states[lane] = states[lane]
                .wrapping_mul(frequency)
                .wrapping_add(u64::from(remainder));
            budgets[lane] += log;
            *out = symbol;
        }
        if position != input.len() {
            return Err(Error::TrailingInput);
        }
        if states.iter().any(|&n| n != 0) {
            return Err(Error::InvalidState);
        }
        Ok(())
    }
}
