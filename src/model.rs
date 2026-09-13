use crate::Error;

pub const PROBABILITY_TOTAL: u32 = 65536;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Symbol {
    pub frequency: u32,
    #[cfg_attr(feature = "reference-division", allow(dead_code))]
    pub reciprocal: u64,
    first_segment: u32,
    segment_count: u32,
    encode_base: u32,
}

#[derive(Clone, Copy, Debug)]
struct Slot {
    symbol: u32,
    frequency: u32,
    adjustment: i32,
}

#[derive(Clone, Copy, Debug)]
struct Bucket {
    cutoff: u32,
    left: Slot,
    right: Slot,
}

#[derive(Clone, Copy, Debug)]
struct Segment {
    end: u32,
    adjustment: i32,
}

/// One code word's decoded symbol and its information-buffer contribution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodedSymbol {
    pub symbol: u32,
    pub frequency: u32,
    pub remainder: u32,
}

/// Optional speed/memory tradeoffs. Tables do not change payload bytes.
#[derive(Clone, Copy, Debug, Default)]
pub struct TableOptions {
    /// Additional 128 KiB/model for direct remainder-to-code lookup.
    pub direct_encode: bool,
    /// Additional 512 KiB/model for direct code-to-symbol lookup.
    pub direct_decode: bool,
}

/// An immutable 16-bit probability model. Zero-frequency symbol IDs are kept.
/// Construction uses the original Blitzcrank alias ordering for interoperability.
#[derive(Clone, Debug)]
pub struct Model {
    pub(crate) symbols: Vec<Symbol>,
    #[cfg(not(feature = "flat-alias"))]
    buckets: Vec<Bucket>,
    #[cfg(feature = "flat-alias")]
    cutoffs: Box<[u32]>,
    #[cfg(feature = "flat-alias")]
    slots: Box<[Slot]>,
    segments: Vec<Segment>,
    shift: u32,
    mask: u32,
    encode_table: Option<Box<[u16]>>,
    decode_table: Option<Box<[u64]>>,
}

impl Model {
    pub fn new(frequencies: &[u32]) -> Result<Self, Error> {
        if frequencies.is_empty()
            || frequencies.len() > PROBABILITY_TOTAL as usize
            || frequencies.iter().map(|&f| u64::from(f)).sum::<u64>()
                != u64::from(PROBABILITY_TOTAL)
        {
            return Err(Error::InvalidModel);
        }
        let active = frequencies.iter().filter(|&&f| f != 0).count();
        let bucket_count = active.next_power_of_two();
        let shift = 16 - bucket_count.trailing_zeros();
        let bucket_size = 1u32 << shift;
        let mut small = Vec::new();
        let mut large = Vec::new();
        for (i, &frequency) in frequencies.iter().enumerate() {
            if frequency != 0 {
                let list = if frequency < bucket_size {
                    &mut small
                } else {
                    &mut large
                };
                list.push((frequency, i as u32));
            }
        }
        let zero = Slot {
            symbol: 0,
            frequency: 0,
            adjustment: 0,
        };
        let mut buckets = vec![
            Bucket {
                cutoff: 0,
                left: zero,
                right: zero,
            };
            bucket_count
        ];
        for bucket in buckets.iter_mut().rev() {
            let (right_weight, right_symbol) = large.pop().ok_or(Error::InvalidModel)?;
            let (left_weight, left_symbol) = small.pop().unwrap_or((0, right_symbol));
            *bucket = Bucket {
                cutoff: left_weight,
                left: Slot {
                    symbol: left_symbol,
                    frequency: frequencies[left_symbol as usize],
                    adjustment: 0,
                },
                right: Slot {
                    symbol: right_symbol,
                    frequency: frequencies[right_symbol as usize],
                    adjustment: 0,
                },
            };
            let remaining = right_weight - (bucket_size - left_weight);
            let list = if remaining < bucket_size {
                &mut small
            } else {
                &mut large
            };
            list.push((remaining, right_symbol));
        }
        let mut assigned = vec![0u32; frequencies.len()];
        let mut lists: Vec<Vec<Segment>> = vec![Vec::new(); frequencies.len()];
        let mut position = 0u32;
        for bucket in &mut buckets {
            for (slot, weight) in [
                (&mut bucket.left, bucket.cutoff),
                (&mut bucket.right, bucket_size - bucket.cutoff),
            ] {
                let id = slot.symbol as usize;
                slot.adjustment = position as i32 - assigned[id] as i32;
                if weight != 0 {
                    let list = &mut lists[id];
                    if let Some(previous) =
                        list.last_mut().filter(|s| s.adjustment == slot.adjustment)
                    {
                        previous.end += weight;
                    } else {
                        list.push(Segment {
                            end: assigned[id] + weight,
                            adjustment: slot.adjustment,
                        });
                    }
                }
                assigned[id] += weight;
                position += weight;
            }
        }
        debug_assert_eq!(assigned, frequencies);
        let mut symbols = Vec::with_capacity(frequencies.len());
        let mut segments = Vec::new();
        let mut encode_base = 0;
        for (&frequency, list) in frequencies.iter().zip(lists) {
            symbols.push(Symbol {
                frequency,
                reciprocal: if frequency > 1 {
                    u64::MAX / u64::from(frequency) + 1
                } else {
                    0
                },
                first_segment: segments.len() as u32,
                segment_count: list.len() as u32,
                encode_base,
            });
            encode_base += frequency;
            segments.extend(list);
        }
        Ok(Self {
            symbols,
            #[cfg(feature = "flat-alias")]
            cutoffs: buckets.iter().map(|b| b.cutoff).collect(),
            #[cfg(feature = "flat-alias")]
            slots: buckets
                .into_iter()
                .flat_map(|b| [b.left, b.right])
                .collect(),
            #[cfg(not(feature = "flat-alias"))]
            buckets,
            segments,
            shift,
            mask: bucket_size - 1,
            encode_table: None,
            decode_table: None,
        })
    }

    /// Choose table strategies during model preparation; finished models remain immutable.
    pub fn with_tables(mut self, options: TableOptions) -> Self {
        let mut encode = options.direct_encode.then(|| vec![0u16; 65536]);
        let mut decode = options.direct_decode.then(|| vec![0u64; 65536]);
        if encode.is_some() || decode.is_some() {
            for word in 0..=u16::MAX {
                let item = self.lookup(word);
                if let Some(table) = &mut encode {
                    let base = self.symbols[item.symbol as usize].encode_base;
                    table[(base + item.remainder) as usize] = word;
                }
                if let Some(table) = &mut decode {
                    table[word as usize] = (u64::from(item.frequency - 1) << 32)
                        | (u64::from(item.symbol) << 16)
                        | u64::from(item.remainder);
                }
            }
        }
        self.encode_table = encode.map(Vec::into_boxed_slice);
        self.decode_table = decode.map(Vec::into_boxed_slice);
        self
    }

    /// Deterministic largest-remainder normalization after reserving one unit
    /// for each observed symbol. Zero counts stay zero. This is a convenience
    /// policy; callers can supply their own normalized frequencies to `new`.
    pub fn normalize(counts: &[u32]) -> Result<Vec<u32>, Error> {
        if counts.is_empty() || counts.len() > PROBABILITY_TOTAL as usize {
            return Err(Error::InvalidModel);
        }
        let total: u64 = counts.iter().map(|&c| u64::from(c)).sum();
        if total == 0 {
            return Err(Error::InvalidModel);
        }
        let active = counts.iter().filter(|&&c| c != 0).count() as u32;
        let remaining = PROBABILITY_TOTAL - active;
        let mut weights = vec![0; counts.len()];
        let mut remainders = Vec::with_capacity(active as usize);
        let mut assigned = 0;
        for (i, &count) in counts.iter().enumerate() {
            if count == 0 {
                continue;
            }
            let scaled = u64::from(count) * u64::from(remaining);
            weights[i] = 1 + (scaled / total) as u32;
            assigned += weights[i];
            remainders.push((scaled % total, i));
        }
        remainders.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        for &(_, i) in remainders
            .iter()
            .take((PROBABILITY_TOTAL - assigned) as usize)
        {
            weights[i] += 1;
        }
        Ok(weights)
    }

    pub fn from_counts(counts: &[u32]) -> Result<Self, Error> {
        Self::new(&Self::normalize(counts)?)
    }
    pub fn frequencies(&self) -> Vec<u32> {
        self.symbols.iter().map(|s| s.frequency).collect()
    }
    pub fn alphabet_size(&self) -> usize {
        self.symbols.len()
    }
    /// Allocated table storage, excluding Vec headers and allocator bookkeeping.
    pub fn table_bytes(&self) -> usize {
        #[cfg(feature = "flat-alias")]
        let compact_bytes = self.cutoffs.len() * std::mem::size_of::<u32>()
            + self.slots.len() * std::mem::size_of::<Slot>();
        #[cfg(not(feature = "flat-alias"))]
        let compact_bytes = self.buckets.capacity() * std::mem::size_of::<Bucket>();
        self.symbols.capacity() * std::mem::size_of::<Symbol>()
            + compact_bytes
            + self.segments.capacity() * std::mem::size_of::<Segment>()
            + self
                .encode_table
                .as_ref()
                .map_or(0, |table| table.len() * 2)
            + self
                .decode_table
                .as_ref()
                .map_or(0, |table| table.len() * 8)
    }

    #[inline]
    pub fn lookup(&self, word: u16) -> DecodedSymbol {
        if let Some(table) = &self.decode_table {
            let entry = table[word as usize];
            return DecodedSymbol {
                symbol: ((entry >> 16) & 65535) as u32,
                frequency: (entry >> 32) as u32 + 1,
                remainder: (entry & 65535) as u32,
            };
        }
        let word = u32::from(word);
        #[cfg(feature = "flat-alias")]
        let slot = {
            let bucket = (word >> self.shift) as usize;
            // Boolean-index addressing trades fixed-model throughput against
            // extra dependent loads in some conditional-model workloads.
            &self.slots[2 * bucket + usize::from(word & self.mask >= self.cutoffs[bucket])]
        };
        #[cfg(not(feature = "flat-alias"))]
        let slot = {
            let bucket = &self.buckets[(word >> self.shift) as usize];
            if word & self.mask < bucket.cutoff {
                &bucket.left
            } else {
                &bucket.right
            }
        };
        DecodedSymbol {
            symbol: slot.symbol,
            frequency: slot.frequency,
            remainder: (word as i32 - slot.adjustment) as u32,
        }
    }

    pub(crate) fn direct_decode_table(&self) -> Option<&[u64; 65536]> {
        self.decode_table
            .as_deref()
            .map(|table| table.try_into().expect("complete decode table"))
    }

    /// Inverse of lookup, useful for testing and custom coding integrations.
    pub fn embed(&self, symbol: u32, remainder: u32) -> Result<u16, Error> {
        let s = self
            .symbols
            .get(symbol as usize)
            .ok_or(Error::InvalidSymbol)?;
        if remainder >= s.frequency {
            return Err(Error::InvalidSymbol);
        }
        Ok(self.embed_validated(s, remainder))
    }

    #[inline]
    pub(crate) fn embed_validated(&self, symbol: &Symbol, remainder: u32) -> u16 {
        if let Some(table) = &self.encode_table {
            return table[(symbol.encode_base + remainder) as usize];
        }
        let segments = &self.segments
            [symbol.first_segment as usize..(symbol.first_segment + symbol.segment_count) as usize];
        if segments.len() > 4 {
            // A frequent symbol may occupy hundreds of disjoint alias segments.
            let index = segments.partition_point(|segment| segment.end <= remainder);
            return (remainder as i32 + segments[index].adjustment) as u16;
        }
        for segment in segments {
            if remainder < segment.end {
                return (remainder as i32 + segment.adjustment) as u16;
            }
        }
        unreachable!("validated remainder must belong to a segment")
    }
}
