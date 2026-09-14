//! Opt-in DC16 block coding, with a portable scalar backend and runtime AVX-512.
//! Cumulative mapping and 64 states are a separate payload contract, not the
//! core alias stream. Caller supplies the model, symbol count and lane count.
//! No probability quantization is performed by this crate.
#![deny(unsafe_op_in_unsafe_fn)]

use delayed_coding::{DecodedSymbol, Error, Model};
use std::ops::Range;

#[derive(Default)]
pub struct EncodeWorkspace {
    masks: Vec<u16>,
}

#[cfg(all(feature = "avx512", target_arch = "x86_64"))]
#[clippy::msrv = "1.89"]
mod encode;

pub struct SimdModel {
    model: Model,
    frequencies: Vec<u32>,
    // P12: remainder/16 (12), frequency/16-1 (12), symbol (8).
    // P16: remainder (16), frequency-1 (16), with separate symbol table.
    entries: Vec<u32>,
    ids: Vec<u32>,
    #[cfg_attr(not(all(feature = "avx512", target_arch = "x86_64")), allow(dead_code))]
    compact: bool,
    cumulative: bool,
    accelerated: bool,
    starts: [u32; 256],
    #[cfg_attr(not(all(feature = "avx512", target_arch = "x86_64")), allow(dead_code))]
    reciprocal: [u32; 256],
}

impl SimdModel {
    pub fn new(frequencies: &[u32]) -> Result<Self, Error> {
        Self::build(frequencies, false, true)
    }

    /// Separate cumulative mapping, not compatible with the alias payload.
    pub fn new_cumulative(frequencies: &[u32]) -> Result<Self, Error> {
        Self::build(frequencies, true, true)
    }

    /// Same format and results, without SIMD dispatch or direct decode tables.
    /// Useful for bounded-memory preparation and differential tests.
    pub fn new_cumulative_scalar(frequencies: &[u32]) -> Result<Self, Error> {
        Self::build(frequencies, true, false)
    }

    fn build(frequencies: &[u32], cumulative: bool, accelerated: bool) -> Result<Self, Error> {
        if frequencies.len() > 256 {
            return Err(Error::InvalidModel);
        }
        let model = Model::new(frequencies)?;
        let mut starts = [65536; 256];
        let mut reciprocal = [0; 256];
        let mut total = 0;
        for (id, &f) in frequencies.iter().enumerate() {
            starts[id] = total;
            total += f;
            if f > 1 {
                reciprocal[id] = ((1u64 << 32) / u64::from(f)) as u32;
            }
        }
        let compact = frequencies.iter().all(|f| f % 16 == 0);
        let accelerated = accelerated && Self::available();
        let size = if !accelerated {
            0
        } else if compact {
            4096
        } else {
            65536
        };
        let mut entries = vec![0; size];
        let mut ids = if compact { vec![] } else { vec![0; size] };
        for word in 0..if accelerated { 65536u32 } else { 0 } {
            let d = if cumulative {
                let id = starts.partition_point(|&s| s <= word) - 1;
                DecodedSymbol {
                    symbol: id as u32,
                    frequency: frequencies[id],
                    remainder: word - starts[id],
                }
            } else {
                model.lookup(word as u16)
            };
            if compact {
                let entry = (d.remainder >> 4) | ((d.frequency / 16 - 1) << 12) | (d.symbol << 24);
                if word % 16 == 0 {
                    entries[(word / 16) as usize] = entry;
                }
                // Verify the alias map's complete 16-word alignment property.
                assert_eq!(entries[(word / 16) as usize], entry);
                assert_eq!(d.remainder & 15, word & 15);
            } else {
                entries[word as usize] = d.remainder | ((d.frequency - 1) << 16);
                ids[word as usize] = d.symbol;
            }
        }
        let mut frequencies = frequencies.to_vec();
        frequencies.resize(256, 0);
        Ok(Self {
            model,
            frequencies,
            entries,
            ids,
            compact,
            cumulative,
            accelerated,
            starts,
            reciprocal,
        })
    }

    fn lookup(&self, word: u16) -> DecodedSymbol {
        if self.cumulative {
            let id = self.starts.partition_point(|&s| s <= u32::from(word)) - 1;
            DecodedSymbol {
                symbol: id as u32,
                frequency: self.frequencies[id],
                remainder: u32::from(word) - self.starts[id],
            }
        } else {
            self.model.lookup(word)
        }
    }

    fn embed(&self, symbol: u32, remainder: u32) -> Result<u16, Error> {
        if self.cumulative {
            Ok((self.starts[symbol as usize] + remainder) as u16)
        } else {
            self.model.embed(symbol, remainder)
        }
    }

    /// Optimized cumulative encoder; includes validation and forward schedule.
    #[inline(never)]
    pub fn encode64_into(
        &self,
        input: &[u32],
        output: &mut [u8],
        workspace: &mut EncodeWorkspace,
    ) -> Result<Range<usize>, Error> {
        if !self.cumulative {
            return Err(Error::InvalidModel);
        }
        input.len().checked_mul(2).ok_or(Error::InputTooLarge)?;
        workspace.masks.resize(input.len().div_ceil(16), 0);
        #[cfg(all(feature = "avx512", target_arch = "x86_64"))]
        if self.accelerated && Self::available() {
            // SAFETY: feature check; all model arrays have exactly 256 entries;
            // encoder validates IDs before gathers and byte count before writes.
            return unsafe { encode::encode64(self, input, output, &mut workspace.masks) };
        }
        // Reuse the same compact schedule as the vector encoder. The portable
        // path must not allocate a second payload or one flag byte per symbol.
        let mut capacities = [1u32; 64];
        let mut count = 0usize;
        for (i, &id) in input.iter().enumerate() {
            let f = *self
                .frequencies
                .get(id as usize)
                .ok_or(Error::InvalidSymbol)?;
            if f == 0 {
                return Err(Error::InvalidSymbol);
            }
            let c = &mut capacities[i % 64];
            let physical = *c < 65536;
            if i % 16 == 0 {
                workspace.masks[i / 16] = 0;
            }
            workspace.masks[i / 16] |= u16::from(physical) << (i % 16);
            count += usize::from(physical);
            if !physical {
                *c >>= 16;
            }
            *c *= f;
        }
        let bytes = count * 2;
        let start = output
            .len()
            .checked_sub(bytes)
            .ok_or(Error::OutputTooSmall)?;
        let mut position = output.len();
        let mut states = [0u32; 64];
        for (i, &id) in input.iter().enumerate().rev() {
            let n = &mut states[i % 64];
            let f = self.frequencies[id as usize];
            let word = self.starts[id as usize] + *n % f;
            let q = *n / f;
            if workspace.masks[i / 16] & (1 << (i % 16)) != 0 {
                position -= 2;
                output[position..position + 2].copy_from_slice(&(word as u16).to_be_bytes());
                *n = q;
            } else {
                *n = (q << 16) | word;
            }
        }
        debug_assert_eq!(position, start);
        debug_assert_eq!(states, [0; 64]);
        Ok(start..output.len())
    }

    /// Hot decoder tables only; the retained scalar oracle model is excluded.
    pub fn decode_table_bytes(&self) -> usize {
        (self.entries.len() + self.ids.len()) * 4
    }

    pub fn backend(&self) -> &'static str {
        if self.accelerated {
            "avx512"
        } else {
            "scalar"
        }
    }

    /// Owned allocation estimate; excludes object headers and allocator overhead.
    pub fn memory_bytes(&self) -> usize {
        self.decode_table_bytes() + self.scalar_table_bytes() + self.frequencies.capacity() * 4
    }

    pub fn scalar_table_bytes(&self) -> usize {
        self.model.table_bytes()
    }

    /// Independent, division-based scalar encoder. Not speed-optimized.
    pub fn encode<const L: usize>(&self, input: &[u32]) -> Result<Vec<u8>, Error> {
        self.encode_delay::<16, L>(input)
    }

    /// Scalar rate-control oracle: same lanes/mapping with another delay.
    pub fn encode_delay<const D: u32, const L: usize>(
        &self,
        input: &[u32],
    ) -> Result<Vec<u8>, Error> {
        if !(16..=32).contains(&D) {
            return Err(Error::InvalidDelay);
        }
        if !matches!(L, 1 | 2 | 4 | 8 | 16 | 32 | 64) {
            return Err(Error::InvalidLanes);
        }
        let mut capacity = [1u64; L];
        let mut flags = Vec::with_capacity(input.len());
        let mut count = 0usize;
        for (i, &id) in input.iter().enumerate() {
            let f = *self
                .frequencies
                .get(id as usize)
                .ok_or(Error::InvalidSymbol)?;
            if f == 0 {
                return Err(Error::InvalidSymbol);
            }
            let c = &mut capacity[i % L];
            let virtual_word = *c >= 1u64 << D;
            flags.push(virtual_word);
            if virtual_word {
                *c >>= 16;
            } else {
                count += 1;
            }
            *c *= u64::from(f);
            assert!(*c < 1u64 << (D + 16));
        }
        let mut output = vec![0; count.checked_mul(2).ok_or(Error::InputTooLarge)?];
        let mut position = output.len();
        let mut states = [0u64; L];
        for (i, &id) in input.iter().enumerate().rev() {
            let n = &mut states[i % L];
            let f = u64::from(self.frequencies[id as usize]);
            let word = self.embed(id, (*n % f) as u32)?;
            *n /= f;
            if flags[i] {
                *n = (*n << 16) | u64::from(word);
            } else {
                position -= 2;
                output[position..position + 2].copy_from_slice(&word.to_be_bytes());
            }
        }
        assert_eq!(states, [0; L]);
        Ok(output)
    }

    /// Scalar u64 arithmetic / original alias lookup oracle, including tails.
    pub fn decode_scalar<const L: usize>(
        &self,
        input: &[u8],
        output: &mut [u32],
    ) -> Result<(), Error> {
        if !matches!(L, 1 | 2 | 4 | 8 | 16 | 32 | 64) {
            return Err(Error::InvalidLanes);
        }
        self.finish::<L>(input, output, 0, 0, [0; L], [1; L])
    }

    #[allow(clippy::too_many_arguments)]
    fn finish<const L: usize>(
        &self,
        input: &[u8],
        output: &mut [u32],
        mut position: usize,
        completed: usize,
        mut states: [u32; L],
        mut capacities: [u32; L],
    ) -> Result<(), Error> {
        for (i, out) in output.iter_mut().enumerate().skip(completed) {
            let lane = i % L;
            let mut n = u64::from(states[lane]);
            let mut c = u64::from(capacities[lane]);
            let word = if c >= 65536 {
                c >>= 16;
                let w = n as u16;
                n >>= 16;
                w
            } else {
                let bytes = input
                    .get(position..position + 2)
                    .ok_or(Error::TruncatedInput)?;
                position += 2;
                u16::from_be_bytes([bytes[0], bytes[1]])
            };
            if n >= c {
                return Err(Error::InvalidState);
            }
            let d = self.lookup(word);
            n = n * u64::from(d.frequency) + u64::from(d.remainder);
            c *= u64::from(d.frequency);
            assert!(n < c && c < 1u64 << 32);
            states[lane] = n as u32;
            capacities[lane] = c as u32;
            *out = d.symbol;
        }
        if position != input.len() {
            return Err(Error::TrailingInput);
        }
        if states.iter().any(|&n| n != 0) {
            return Err(Error::InvalidState);
        }
        Ok(())
    }

    pub fn available() -> bool {
        #[cfg(all(feature = "avx512", target_arch = "x86_64"))]
        {
            std::is_x86_feature_detected!("avx512f")
                && std::is_x86_feature_detected!("avx2")
                && std::is_x86_feature_detected!("avx512bw")
                && std::is_x86_feature_detected!("avx512vl")
                && std::is_x86_feature_detected!("avx512vbmi2")
                && std::is_x86_feature_detected!("popcnt")
        }
        #[cfg(not(all(feature = "avx512", target_arch = "x86_64")))]
        {
            false
        }
    }

    /// Runtime-dispatched, fully bounded input. No padding requirement.
    #[inline(never)]
    pub fn decode16(&self, input: &[u8], output: &mut [u32]) -> Result<(), Error> {
        self.dispatch::<1, 16, false>(input, output)
    }

    #[inline(never)]
    pub fn decode32(&self, input: &[u8], output: &mut [u32]) -> Result<(), Error> {
        self.dispatch::<2, 32, false>(input, output)
    }

    #[inline(never)]
    pub fn decode64(&self, input: &[u8], output: &mut [u32]) -> Result<(), Error> {
        self.dispatch::<4, 64, false>(input, output)
    }

    /// Masked 16-bit input expansion: remains SIMD even with no bytes remaining.
    #[inline(never)]
    pub fn decode64_bounded(&self, input: &[u8], output: &mut [u32]) -> Result<(), Error> {
        self.dispatch::<4, 64, true>(input, output)
    }

    fn dispatch<const G: usize, const L: usize, const BOUNDED: bool>(
        &self,
        input: &[u8],
        output: &mut [u32],
    ) -> Result<(), Error> {
        #[cfg(all(feature = "avx512", target_arch = "x86_64"))]
        if self.accelerated && Self::available() {
            // SAFETY: runtime ISA check; private tables constructed and validated
            // above; kernel checks every input/output window and masks indices.
            return unsafe {
                if self.compact {
                    x86::decode::<true, G, L, BOUNDED>(self, input, output)
                } else {
                    x86::decode::<false, G, L, BOUNDED>(self, input, output)
                }
            };
        }
        self.decode_scalar::<L>(input, output)
    }
}

#[cfg(all(feature = "avx512", target_arch = "x86_64"))]
#[clippy::msrv = "1.89"]
mod x86 {
    use super::*;
    use std::arch::x86_64::*;

    #[target_feature(enable = "avx512f,avx2,avx512bw,avx512vl,avx512vbmi2,popcnt")]
    pub unsafe fn decode<
        const COMPACT: bool,
        const G: usize,
        const L: usize,
        const BOUNDED: bool,
    >(
        model: &SimdModel,
        input: &[u8],
        output: &mut [u32],
    ) -> Result<(), Error> {
        // SAFETY: feature detection in caller. Window mode checks G*32 readable
        // bytes; bounded mode checks each mask's exact physical byte count.
        // Each group requires 16 writable u32s. Gather indices are masked into
        // private 4096/65536-entry tables, independent of stream validity.
        unsafe {
            assert_eq!(L, G * 16);
            assert!(matches!(G, 1 | 2 | 4));
            let mut n = [_mm512_setzero_si512(); G];
            let mut c = [_mm512_set1_epi32(1); G];
            let threshold = _mm512_set1_epi32(65536);
            let mask16 = _mm512_set1_epi32(65535);
            let mask12 = _mm512_set1_epi32(4095);
            let swap = _mm256_setr_epi8(
                1, 0, 3, 2, 5, 4, 7, 6, 9, 8, 11, 10, 13, 12, 15, 14, 1, 0, 3, 2, 5, 4, 7, 6, 9, 8,
                11, 10, 13, 12, 15, 14,
            );
            let mut position = 0;
            let mut completed = 0;
            while output.len() - completed >= L && (BOUNDED || input.len() - position >= L * 2) {
                macro_rules! group {
                    ($group:expr) => {{
                        let group = $group;
                        let physical = _mm512_cmplt_epu32_mask(c[group], threshold);
                        // Expand consecutive physical words into precisely the lanes
                        // that need input; virtual lanes retain the numerator low word.
                        let words = if BOUNDED {
                            let needed = physical.count_ones() as usize * 2;
                            if input.len() - position < needed {
                                return Err(Error::TruncatedInput);
                            }
                            // Masked expand reads exactly popcount(mask) consecutive
                            // words, never a full window or inaccessible padding.
                            let expanded = _mm256_maskz_expandloadu_epi16(
                                physical,
                                input.as_ptr().add(position).cast(),
                            );
                            let expanded =
                                _mm512_cvtepu16_epi32(_mm256_shuffle_epi8(expanded, swap));
                            _mm512_and_si512(
                                _mm512_mask_mov_epi32(n[group], physical, expanded),
                                mask16,
                            )
                        } else {
                            let dense = _mm256_loadu_si256(input.as_ptr().add(position).cast());
                            let dense = _mm512_cvtepu16_epi32(_mm256_shuffle_epi8(dense, swap));
                            _mm512_and_si512(
                                _mm512_mask_expand_epi32(n[group], physical, dense),
                                mask16,
                            )
                        };
                        let normalized_n =
                            _mm512_mask_srli_epi32::<16>(n[group], !physical, n[group]);
                        let normalized_c =
                            _mm512_mask_srli_epi32::<16>(c[group], !physical, c[group]);
                        // Establish the 32-bit multiplication bound even on corrupt input.
                        if _mm512_cmpge_epu32_mask(normalized_n, normalized_c) != 0 {
                            return Err(Error::InvalidState);
                        }
                        let (frequency, remainder, ids) = if COMPACT {
                            let index = _mm512_srli_epi32::<4>(words);
                            let entry =
                                _mm512_i32gather_epi32::<4>(index, model.entries.as_ptr().cast());
                            let f = _mm512_slli_epi32::<4>(_mm512_add_epi32(
                                _mm512_and_si512(_mm512_srli_epi32::<12>(entry), mask12),
                                _mm512_set1_epi32(1),
                            ));
                            let r = _mm512_or_si512(
                                _mm512_slli_epi32::<4>(_mm512_and_si512(entry, mask12)),
                                _mm512_and_si512(words, _mm512_set1_epi32(15)),
                            );
                            (f, r, _mm512_srli_epi32::<24>(entry))
                        } else {
                            let entry =
                                _mm512_i32gather_epi32::<4>(words, model.entries.as_ptr().cast());
                            let ids = _mm512_i32gather_epi32::<4>(words, model.ids.as_ptr().cast());
                            (
                                _mm512_add_epi32(
                                    _mm512_srli_epi32::<16>(entry),
                                    _mm512_set1_epi32(1),
                                ),
                                _mm512_and_si512(entry, mask16),
                                ids,
                            )
                        };
                        n[group] = _mm512_add_epi32(
                            _mm512_mullo_epi32(normalized_n, frequency),
                            remainder,
                        );
                        c[group] = _mm512_mullo_epi32(normalized_c, frequency);
                        _mm512_storeu_si512(output.as_mut_ptr().add(completed).cast(), ids);
                        position += physical.count_ones() as usize * 2;
                        completed += 16;
                    }};
                }
                group!(0);
                if G >= 2 {
                    group!(1);
                }
                if G >= 3 {
                    group!(2);
                }
                if G >= 4 {
                    group!(3);
                }
            }
            let mut states = [0u32; L];
            let mut capacities = [0u32; L];
            for group in 0..G {
                _mm512_storeu_si512(states.as_mut_ptr().add(group * 16).cast(), n[group]);
                _mm512_storeu_si512(capacities.as_mut_ptr().add(group * 16).cast(), c[group]);
            }
            model.finish::<L>(input, output, position, completed, states, capacities)
        }
    }
}

#[cfg(test)]
mod tests;
