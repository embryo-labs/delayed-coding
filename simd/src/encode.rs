use super::*;
use std::arch::x86_64::*;

// floor(n/f) using a downward-rounded 32-bit reciprocal, then at most one
// correction. For n<2^32 the quotient underestimate is strictly less than one
// before flooring. f=1 is selected separately, without reciprocal overflow.
#[target_feature(enable = "avx512f,avx2")]
unsafe fn divide(n: __m512i, f: __m512i, reciprocal: __m512i) -> (__m512i, __m512i) {
    let even = _mm512_mul_epu32(n, reciprocal);
    let odd = _mm512_mul_epu32(
        _mm512_srli_epi64::<32>(n),
        _mm512_srli_epi64::<32>(reciprocal),
    );
    let q0 = _mm512_mask_blend_epi32(0xaaaa, _mm512_srli_epi64::<32>(even), odd);
    let r0 = _mm512_sub_epi32(n, _mm512_mullo_epi32(q0, f));
    let correction = _mm512_cmpge_epu32_mask(r0, f);
    let q = _mm512_mask_add_epi32(q0, correction, q0, _mm512_set1_epi32(1));
    let r = _mm512_mask_sub_epi32(r0, correction, r0, f);
    let one = _mm512_cmpeq_epi32_mask(f, _mm512_set1_epi32(1));
    (
        _mm512_mask_mov_epi32(q, one, n),
        _mm512_mask_mov_epi32(r, one, _mm512_setzero_si512()),
    )
}

#[target_feature(enable = "avx512f,avx2,popcnt")]
pub(super) unsafe fn encode64(
    model: &SimdModel,
    input: &[u32],
    output: &mut [u8],
    masks: &mut [u16],
) -> Result<Range<usize>, Error> {
    // SAFETY: caller checks ISA and allocates ceil(input.len()/16) masks.
    // Every SIMD ID load is bounded, IDs are checked before table gathers,
    // exact output size is checked before any store, including masked stores.
    unsafe {
        let full = input.len() / 64 * 64;
        let threshold = _mm512_set1_epi32(65536);
        let mut capacity = [_mm512_set1_epi32(1); 4];
        let mut count = 0usize;
        for base in (0..full).step_by(64) {
            for (group, c) in capacity.iter_mut().enumerate() {
                let offset = base + group * 16;
                let ids = _mm512_loadu_si512(input.as_ptr().add(offset).cast());
                if _mm512_cmpge_epu32_mask(ids, _mm512_set1_epi32(256)) != 0 {
                    return Err(Error::InvalidSymbol);
                }
                let f = _mm512_i32gather_epi32::<4>(ids, model.frequencies.as_ptr().cast());
                if _mm512_cmpeq_epi32_mask(f, _mm512_setzero_si512()) != 0 {
                    return Err(Error::InvalidSymbol);
                }
                let physical = _mm512_cmplt_epu32_mask(*c, threshold);
                masks[offset / 16] = physical;
                count += physical.count_ones() as usize;
                *c = _mm512_mullo_epi32(_mm512_mask_srli_epi32::<16>(*c, !physical, *c), f);
            }
        }
        let mut tail_c = [0u32; 64];
        for (group, &c) in capacity.iter().enumerate() {
            _mm512_storeu_si512(tail_c.as_mut_ptr().add(group * 16).cast(), c);
        }
        for i in full..input.len() {
            let id = input[i] as usize;
            let f = *model.frequencies.get(id).ok_or(Error::InvalidSymbol)?;
            if f == 0 {
                return Err(Error::InvalidSymbol);
            }
            let c = &mut tail_c[i % 64];
            let physical = *c < 65536;
            if i % 16 == 0 {
                masks[i / 16] = 0;
            }
            masks[i / 16] |= u16::from(physical) << (i % 16);
            count += usize::from(physical);
            if !physical {
                *c >>= 16;
            }
            *c *= f;
        }
        let bytes = count * 2;
        if bytes > output.len() {
            return Err(Error::OutputTooSmall);
        }
        let mut position = output.len();
        let mut tail_n = [0u32; 64];
        for i in (full..input.len()).rev() {
            let id = input[i] as usize;
            let f = model.frequencies[id];
            let n = &mut tail_n[i % 64];
            let q = *n / f;
            let word = model.starts[id] + *n % f;
            if masks[i / 16] & (1 << (i % 16)) != 0 {
                position -= 2;
                output[position..position + 2].copy_from_slice(&(word as u16).to_be_bytes());
                *n = q;
            } else {
                *n = (q << 16) | word;
            }
        }
        let mut states = [_mm512_setzero_si512(); 4];
        for (group, n) in states.iter_mut().enumerate() {
            *n = _mm512_loadu_si512(tail_n.as_ptr().add(group * 16).cast());
        }
        for base in (0..full).step_by(64).rev() {
            for group in (0..4).rev() {
                let offset = base + group * 16;
                let ids = _mm512_loadu_si512(input.as_ptr().add(offset).cast());
                let f = _mm512_i32gather_epi32::<4>(ids, model.frequencies.as_ptr().cast());
                let reciprocal = _mm512_i32gather_epi32::<4>(ids, model.reciprocal.as_ptr().cast());
                let start = _mm512_i32gather_epi32::<4>(ids, model.starts.as_ptr().cast());
                let (q, r) = divide(states[group], f, reciprocal);
                let word = _mm512_add_epi32(start, r);
                let embedded = _mm512_or_si512(_mm512_slli_epi32::<16>(q), word);
                let physical = masks[offset / 16];
                states[group] = _mm512_mask_mov_epi32(embedded, physical, q);
                let packed = _mm512_maskz_compress_epi32(physical, word);
                let swapped = _mm512_or_si512(
                    _mm512_slli_epi32::<8>(_mm512_and_si512(packed, _mm512_set1_epi32(255))),
                    _mm512_srli_epi32::<8>(packed),
                );
                let words = physical.count_ones() as usize;
                position -= words * 2;
                let store_mask = ((1u32 << words) - 1) as u16;
                _mm512_mask_cvtepi32_storeu_epi16(
                    output.as_mut_ptr().add(position).cast(),
                    store_mask,
                    swapped,
                );
            }
        }
        debug_assert_eq!(position, output.len() - bytes);
        debug_assert!(states
            .iter()
            .all(|&n| _mm512_cmpeq_epi32_mask(n, _mm512_setzero_si512()) == u16::MAX));
        Ok(position..output.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn division_all_frequencies_and_u32_boundaries() {
        if !SimdModel::available() {
            return;
        }
        // SAFETY: detected ISA; full 16-element buffers for every load/store.
        unsafe {
            check_division();
        }
    }

    #[target_feature(enable = "avx512f,avx2")]
    unsafe fn check_division() {
        unsafe {
            let mut rng = 11u32;
            for f in 1..=65536u32 {
                let mut values = [0u32; 16];
                values[..8].copy_from_slice(&[
                    0,
                    1,
                    f - 1,
                    f,
                    f + 1,
                    u32::MAX,
                    u32::MAX - f,
                    u32::MAX / 2,
                ]);
                for n in &mut values[8..] {
                    rng ^= rng << 13;
                    rng ^= rng >> 17;
                    rng ^= rng << 5;
                    *n = rng;
                }
                let reciprocal = if f == 1 {
                    0
                } else {
                    ((1u64 << 32) / u64::from(f)) as u32
                };
                let (q, r) = divide(
                    _mm512_loadu_si512(values.as_ptr().cast()),
                    _mm512_set1_epi32(f as i32),
                    _mm512_set1_epi32(reciprocal as i32),
                );
                let mut quotients = [0u32; 16];
                let mut remainders = [0u32; 16];
                _mm512_storeu_si512(quotients.as_mut_ptr().cast(), q);
                _mm512_storeu_si512(remainders.as_mut_ptr().cast(), r);
                for i in 0..16 {
                    assert_eq!(quotients[i], values[i] / f);
                    assert_eq!(remainders[i], values[i] % f);
                }
            }
        }
    }
}
