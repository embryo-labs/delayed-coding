use crate::{DecodeModel, Decoder, Error};

/// One symbol per prepared model, in event order. Useful for independent
/// heterogeneous records. Keeps the entropy loop separate from row materialization.
#[inline(never)]
pub fn decode_prepared_into<const DELAY: u32, const LANES: usize>(
    models: &[DecodeModel],
    input: &[u8],
    output: &mut [u32],
) -> Result<(), Error> {
    if models.len() != output.len() {
        return Err(Error::OutputTooSmall);
    }
    if DELAY == 16 && matches!(LANES, 1 | 4) {
        return decode16::<LANES>(models, input, output);
    }
    let mut decoder = Decoder::<DELAY, LANES>::new(input)?;
    let mut i = 0;
    if LANES == 4 {
        while i + 4 <= output.len() {
            let m = &models[i..i + 4];
            output[i..i + 4]
                .copy_from_slice(&decoder.read_prepared4([&m[0], &m[1], &m[2], &m[3]])?);
            i += 4;
        }
    }
    for (model, symbol) in models[i..].iter().zip(&mut output[i..]) {
        *symbol = decoder.read_prepared(model)?;
    }
    decoder.finish()
}

// With DELAY=16, a normalized capacity is <2^16 and a frequency is <=2^16.
// Consequently valid information/capacity states fit u32, including before
// renormalization. Narrow states permit four ordinary packed u32 multiplies.
fn decode16<const LANES: usize>(
    models: &[DecodeModel],
    input: &[u8],
    output: &mut [u32],
) -> Result<(), Error> {
    let mut numerator = [0u32; LANES];
    let mut denominator = [1u32; LANES];
    let mut position = 0;
    let mut i = 0;
    if LANES == 4 {
        while i + 4 <= models.len() && input.len() - position >= 8 {
            let physical: [usize; 4] = std::array::from_fn(|j| usize::from(denominator[j] < 65536));
            let offsets = [
                0,
                physical[0],
                physical[0] + physical[1],
                physical[0] + physical[1] + physical[2],
            ];
            let window = &input[position..position + 8];
            let mut words = [0; 4];
            for j in 0..4 {
                let loaded =
                    u16::from_be_bytes([window[2 * offsets[j]], window[2 * offsets[j] + 1]]);
                let mask = (physical[j] as u16).wrapping_neg();
                words[j] = (loaded & mask) | (numerator[j] as u16 & !mask);
                numerator[j] >>= (1 - physical[j]) * 16;
                denominator[j] >>= (1 - physical[j]) * 16;
            }
            let decoded: [_; 4] = std::array::from_fn(|j| models[i + j].lookup(words[j]));
            for j in 0..4 {
                numerator[j] = numerator[j]
                    .wrapping_mul(decoded[j].frequency)
                    .wrapping_add(decoded[j].remainder);
                denominator[j] *= decoded[j].frequency;
                output[i + j] = decoded[j].symbol;
            }
            position += 2 * (offsets[3] + physical[3]);
            i += 4;
        }
    }
    while i < models.len() {
        let lane = i & (LANES - 1);
        let word = if denominator[lane] >= 65536 {
            let word = numerator[lane] as u16;
            numerator[lane] >>= 16;
            denominator[lane] >>= 16;
            word
        } else {
            let bytes = input
                .get(position..position.saturating_add(2))
                .ok_or(Error::TruncatedInput)?;
            position += 2;
            u16::from_be_bytes([bytes[0], bytes[1]])
        };
        let d = models[i].lookup(word);
        numerator[lane] = numerator[lane]
            .wrapping_mul(d.frequency)
            .wrapping_add(d.remainder);
        denominator[lane] *= d.frequency;
        output[i] = d.symbol;
        i += 1;
    }
    if position != input.len() {
        return Err(Error::TrailingInput);
    }
    if numerator.iter().any(|&n| n != 0) {
        return Err(Error::InvalidState);
    }
    Ok(())
}
