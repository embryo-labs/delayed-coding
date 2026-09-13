//! Optional machine kernels. All unchecked accesses are confined here.
#![deny(unsafe_op_in_unsafe_fn)]

pub struct Progress {
    pub states: [u64; 4],
    pub capacities: [u64; 4],
    pub input_position: usize,
    pub output_count: usize,
}

pub fn decode4<const LOG: bool>(table: &[u64; 65536], input: &[u8], output: &mut [u32]) -> Option<Progress> {
    #[cfg(target_arch = "x86_64")]
    if std::is_x86_feature_detected!("avx2") {
        // SAFETY: runtime feature detection; the callee checks every buffer
        // window and masks every gather index to the fixed table's domain.
        return Some(unsafe { x86::decode4::<LOG>(table, input, output) });
    }
    let _ = (table, input, output);
    None
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    use super::Progress;
    use std::arch::x86_64::*;

    const fn shuffles() -> [[u8; 16]; 16] {
        let mut all = [[0x80; 16]; 16];
        let mut mask = 0;
        while mask < 16 {
            let mut offset = 0;
            let mut lane = 0;
            while lane < 4 {
                all[mask][lane*4] = offset*2+1;
                all[mask][lane*4+1] = offset*2;
                offset += ((mask >> lane)&1) as u8;
                lane += 1;
            }
            mask += 1;
        }
        all
    }
    static SHUFFLES: [[u8; 16]; 16] = shuffles();

    #[target_feature(enable = "avx2")]
    pub unsafe fn decode4<const LOG: bool>(table: &[u64; 65536], input: &[u8], output: &mut [u32]) -> Progress {
        // SAFETY: caller checked AVX2. Every intrinsic pointer access below is
        // bounded independently of stream/model contents; no padding is read.
        unsafe {
            let mut states = _mm256_setzero_si256();
            let mut capacities = _mm256_set1_epi64x(if LOG { 0 } else { 1 });
            let threshold = _mm256_set1_epi64x(if LOG { 6144 } else { 1<<24 });
            let mask16 = _mm256_set1_epi64x(65535);
            let mut position = 0;
            let mut completed = 0;
            while output.len()-completed >= 4 && input.len()-position >= 8 {
                let physical = _mm256_cmpgt_epi64(threshold, capacities);
                let bits = _mm256_movemask_pd(_mm256_castsi256_pd(physical)) as usize;
                // bits is a four-bit mask. Load exactly eight readable bytes;
                // shuffle only accesses the loaded vector (unused bytes zero).
                let words = _mm_loadl_epi64(input.as_ptr().add(position).cast());
                let shuffle = _mm_loadu_si128(SHUFFLES[bits].as_ptr().cast());
                let words = _mm256_cvtepu32_epi64(_mm_shuffle_epi8(words, shuffle));
                let codes = _mm256_and_si256(_mm256_blendv_epi8(states, words, physical), mask16);
                let normalized = _mm256_blendv_epi8(_mm256_srli_epi64::<16>(states), states, physical);
                // Valid delay-24 numerators fit 24 bits before multiplication.
                // If malformed data breaks the weaker 32-bit bound, hand off
                // before this group so the scalar wrapping semantics apply.
                let high = _mm256_srli_epi64::<32>(normalized);
                if _mm256_testz_si256(high, high) == 0 { break; }
                let entries = _mm256_i64gather_epi64::<8>(table.as_ptr().cast(), codes);
                let frequencies = _mm256_add_epi64(_mm256_and_si256(_mm256_srli_epi64::<32>(entries), mask16), _mm256_set1_epi64x(1));
                states = _mm256_add_epi64(_mm256_mul_epu32(normalized, frequencies), _mm256_and_si256(entries, mask16));
                if LOG {
                    let renorm = _mm256_andnot_si256(physical, _mm256_set1_epi64x(4098));
                    capacities = _mm256_sub_epi64(_mm256_add_epi64(capacities, _mm256_srli_epi64::<48>(entries)), renorm);
                } else {
                    let normalized = _mm256_blendv_epi8(_mm256_srli_epi64::<16>(capacities), capacities, physical);
                    capacities = _mm256_mul_epu32(normalized, frequencies);
                }
                let ids = _mm256_srli_epi64::<16>(entries);
                let ids = _mm256_permutevar8x32_epi32(ids, _mm256_setr_epi32(0,2,4,6,0,0,0,0));
                let ids = _mm_and_si128(_mm256_castsi256_si128(ids), _mm_set1_epi32(65535));
                _mm_storeu_si128(output.as_mut_ptr().add(completed).cast(), ids);
                position += bits.count_ones() as usize * 2;
                completed += 4;
            }
            let mut result = Progress { states: [0; 4], capacities: [0; 4], input_position: position, output_count: completed };
            _mm256_storeu_si256(result.states.as_mut_ptr().cast(), states);
            _mm256_storeu_si256(result.capacities.as_mut_ptr().cast(), capacities);
            result
        }
    }
}
