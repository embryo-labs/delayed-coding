use crate::{codec::quotient_frequency, Error};

/// A half-open interval of 16-bit code words. End 65536 is representable.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Interval {
    pub start: u32,
    pub end: u32,
}

/// One selected encoding branch, possibly spanning disjoint intervals.
///
/// This imports a semantic model's existing mapping without rebuilding a whole
/// alphabet. Intervals must be nonempty, sorted, disjoint and within 0..65536.
/// A single interval is stored inline; no identity table is needed for raw words.
/// Construction computes the reciprocal once, outside the encoding hot loop.
#[derive(Clone, Debug)]
pub struct Branch {
    frequency: u32,
    reciprocal: u64,
    first: Interval,
    // (cumulative remainder end, start minus preceding cumulative length).
    rest: Box<[(u32, u32)]>,
}

impl Branch {
    pub fn new(intervals: &[Interval]) -> Result<Self, Error> {
        if intervals.len() > 65536 {
            return Err(Error::InvalidModel);
        }
        let first = *intervals.first().ok_or(Error::InvalidModel)?;
        let (mut previous, mut frequency) = (0, 0);
        let mut rest = Vec::with_capacity(intervals.len().saturating_sub(1));
        for (i, interval) in intervals.iter().enumerate() {
            if interval.start < previous || interval.start >= interval.end || interval.end > 65536 {
                return Err(Error::InvalidModel);
            }
            let adjustment = interval.start - frequency;
            frequency += interval.end - interval.start;
            if i != 0 {
                rest.push((frequency, adjustment));
            }
            previous = interval.end;
        }
        Ok(Self {
            frequency,
            reciprocal: if frequency > 1 {
                u64::MAX / u64::from(frequency) + 1
            } else {
                0
            },
            first,
            rest: rest.into_boxed_slice(),
        })
    }

    pub fn interval(start: u32, frequency: u32) -> Result<Self, Error> {
        let end = start.checked_add(frequency).ok_or(Error::InvalidModel)?;
        Self::new(&[Interval { start, end }])
    }

    pub fn raw(word: u16) -> Self {
        Self {
            frequency: 1,
            reciprocal: 0,
            first: Interval {
                start: u32::from(word),
                end: u32::from(word) + 1,
            },
            rest: Box::new([]),
        }
    }

    pub fn frequency(&self) -> u32 {
        self.frequency
    }

    /// Exact inverse of the branch's interval order, not a new alias assignment.
    pub fn embed(&self, remainder: u32) -> Result<u16, Error> {
        if remainder >= self.frequency {
            return Err(Error::InvalidSymbol);
        }
        Ok(self.embed_validated(remainder))
    }

    fn embed_validated(&self, remainder: u32) -> u16 {
        if remainder < self.first.end - self.first.start {
            return (self.first.start + remainder) as u16;
        }
        let index = self.rest.partition_point(|&(end, _)| end <= remainder);
        (remainder + self.rest[index].1) as u16
    }

    pub(crate) fn split(&self, numerator: u64) -> (u64, u16) {
        let quotient = quotient_frequency(numerator, self.frequency, self.reciprocal);
        let remainder = (numerator - quotient * u64::from(self.frequency)) as u32;
        (quotient, self.embed_validated(remainder))
    }
}
