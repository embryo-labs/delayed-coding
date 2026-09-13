use crate::Error;

/// Streaming, exact raw-payload layout from realized symbol frequencies alone.
///
/// No information state, symbol mapping or encoded bytes are needed. This is
/// useful when a forward modeling pass must determine output sizes/offsets before
/// backward encoding. It does not make conditional frequencies known in advance
/// at the decoder, or make codeword values independent of the suffix.
///
/// ```
/// use delayed_coding::Layout;
/// let mut layout = Layout::<24>::new()?;
/// let mut offsets = Vec::new();
/// for frequency in [32768, 32768, 32768, 32768] {
///     offsets.push(layout.push_frequency(frequency)?);
/// }
/// assert_eq!(offsets, [Some(0), Some(2), None, None]);
/// assert_eq!(layout.encoded_len(), 4);
/// # Ok::<(), delayed_coding::Error>(())
/// ```
#[derive(Clone, Debug)]
pub struct Layout<const DELAY: u32 = 24, const LANES: usize = 1> {
    capacities: [u64; LANES],
    lane: usize,
    bytes: usize,
}

impl<const DELAY: u32, const LANES: usize> Layout<DELAY, LANES> {
    pub fn new() -> Result<Self, Error> {
        if !(16..=32).contains(&DELAY) {
            return Err(Error::InvalidDelay);
        }
        if !matches!(LANES, 1 | 2 | 4 | 8) {
            return Err(Error::InvalidLanes);
        }
        Ok(Self {
            capacities: [1; LANES],
            lane: 0,
            bytes: 0,
        })
    }

    /// Append one realized frequency (1..=65536). Returns its physical byte
    /// offset, or `None` for a virtual word. Errors leave this layout unchanged.
    /// The frequency must be the integer width used by the actual encoder.
    pub fn push_frequency(&mut self, frequency: u32) -> Result<Option<usize>, Error> {
        if !(1..=65536).contains(&frequency) {
            return Err(Error::InvalidModel);
        }
        if self.capacities[self.lane] < 1u64 << DELAY {
            self.bytes.checked_add(2).ok_or(Error::InputTooLarge)?;
        }
        Ok(self.push_validated(frequency))
    }

    /// Internal encoder path: the immutable model validated frequency, and the
    /// block's checked 2*count bound guarantees that byte accounting fits usize.
    #[inline(always)]
    pub(crate) fn push_validated(&mut self, frequency: u32) -> Option<usize> {
        debug_assert!((1..=65536).contains(&frequency));
        let mut capacity = self.capacities[self.lane];
        let offset = if capacity >= 1u64 << DELAY {
            capacity >>= 16;
            None
        } else {
            let offset = self.bytes;
            self.bytes += 2;
            Some(offset)
        };
        self.capacities[self.lane] = capacity * u64::from(frequency);
        self.lane = (self.lane + 1) & (LANES - 1);
        offset
    }

    /// Exact bytes for the observed prefix encoded as a standalone block.
    /// Excludes external model, symbol count, checksum and framing metadata.
    pub fn encoded_len(&self) -> usize {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_count_overflow_leaves_state_unchanged() {
        let mut layout = Layout::<24>::new().unwrap();
        layout.bytes = usize::MAX - 1;
        assert_eq!(layout.push_frequency(1), Err(Error::InputTooLarge));
        assert_eq!(layout.bytes, usize::MAX - 1);
        assert_eq!(layout.lane, 0);
        assert_eq!(layout.capacities, [1]);
    }
}
