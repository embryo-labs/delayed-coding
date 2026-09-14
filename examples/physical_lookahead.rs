//! Experimental fixed-model decoding: predecode bounded batches of physical words.
//! Run with: cargo run --release --example physical_lookahead -- [count] [file]
use delayed_coding::{
    decode_into, decode_lookahead_into, encode, DecodedSymbol, Error, Model, TableOptions,
};
use std::{hint::black_box, time::Instant};

// Reference experiment, deliberately separate from the default decoder.
fn batched<const BATCH: usize>(
    model: &Model,
    input: &[u8],
    output: &mut [u32],
) -> Result<(), Error> {
    let empty = DecodedSymbol {
        symbol: 0,
        frequency: 0,
        remainder: 0,
    };
    let mut pending = [empty; BATCH];
    let (mut cursor, mut used, mut available) = (0, 0, 0);
    let (mut info, mut capacity) = (0u64, 1u64);
    for symbol in output {
        let decoded = if capacity >= 1 << 24 {
            let decoded = model.lookup(info as u16);
            info >>= 16;
            capacity >>= 16;
            decoded
        } else {
            if used == available {
                available = BATCH.min((input.len() - cursor) / 2);
                if available == 0 {
                    return Err(Error::TruncatedInput);
                }
                // No information-state dependency between these lookups.
                for (slot, bytes) in pending[..available].iter_mut().zip(
                    input[cursor..cursor + available * 2]
                        .as_chunks::<2>()
                        .0
                        .iter(),
                ) {
                    *slot = model.lookup(u16::from_be_bytes([bytes[0], bytes[1]]));
                }
                cursor += available * 2;
                used = 0;
            }
            let decoded = pending[used];
            used += 1;
            decoded
        };
        info = info
            .wrapping_mul(u64::from(decoded.frequency))
            .wrapping_add(u64::from(decoded.remainder));
        capacity *= u64::from(decoded.frequency);
        *symbol = decoded.symbol;
    }
    if cursor != input.len() || used != available {
        return Err(Error::TrailingInput);
    }
    if info != 0 {
        return Err(Error::InvalidState);
    }
    Ok(())
}

fn measure(
    model: &Model,
    input: &[u8],
    expected: &[u32],
    mut run: impl FnMut(&Model, &[u8], &mut [u32]) -> Result<(), Error>,
) -> f64 {
    let mut output = vec![0; expected.len()];
    run(model, input, &mut output).unwrap();
    assert_eq!(expected, output);
    let repeats = (1_048_576 / expected.len()).max(1);
    let mut samples = [0.0; 7];
    for sample in &mut samples {
        let start = Instant::now();
        for _ in 0..repeats {
            run(black_box(model), black_box(input), black_box(&mut output)).unwrap();
            black_box(&output);
        }
        *sample = start.elapsed().as_nanos() as f64 / (repeats * expected.len()) as f64;
    }
    assert_eq!(expected, output);
    samples.sort_by(f64::total_cmp);
    samples[3]
}

fn report(name: &str, model: Model, symbols: &[u32]) {
    let payload = encode::<24>(&model, symbols).unwrap();
    for direct in [false, true] {
        let model = model.clone().with_tables(TableOptions {
            direct_encode: false,
            direct_decode: direct,
        });
        for (kernel, ns) in [
            (
                "serial",
                measure(&model, &payload, symbols, decode_into::<24>),
            ),
            (
                "one_ahead",
                measure(&model, &payload, symbols, decode_lookahead_into::<24>),
            ),
            ("batch1", measure(&model, &payload, symbols, batched::<1>)),
            ("batch8", measure(&model, &payload, symbols, batched::<8>)),
            ("batch32", measure(&model, &payload, symbols, batched::<32>)),
            (
                "batch128",
                measure(&model, &payload, symbols, batched::<128>),
            ),
        ] {
            println!(
                "{name},{},{direct},{kernel},{},{ns:.4}",
                symbols.len(),
                payload.len()
            );
        }
    }
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let count = args.first().map(|s| s.parse().unwrap()).unwrap_or(65536);
    assert!((1..=1 << 26).contains(&count));
    println!("distribution,symbols,direct,kernel,payload_bytes,decode_ns_per_symbol");
    for (name, n) in [
        ("uniform16", 16),
        ("uniform256", 256),
        ("uniform4096", 4096),
        ("skewed", 256),
        ("near_constant", 256),
    ] {
        let mut counts = vec![1; n];
        if name == "skewed" {
            counts[0] = 256;
        }
        if name == "near_constant" {
            counts[0] = 65536;
        }
        let model = Model::from_counts(&counts).unwrap();
        let mut rng = 123456u64;
        let symbols: Vec<_> = (0..count)
            .map(|_| {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                model.lookup(rng as u16).symbol
            })
            .collect();
        report(name, model, &symbols);
    }
    if let Some(path) = args.get(1) {
        let bytes = std::fs::read(path).unwrap();
        assert!(!bytes.is_empty() && bytes.len() <= 1 << 26);
        let mut counts = [0; 256];
        for &byte in &bytes {
            counts[byte as usize] += 1;
        }
        let model = Model::from_counts(&counts).unwrap();
        let symbols: Vec<_> = bytes.iter().map(|&b| u32::from(b)).collect();
        report("file", model, &symbols);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check<const B: usize>(model: &Model, input: &[u8], count: usize) {
        let mut expected = vec![u32::MAX; count];
        let mut actual = expected.clone();
        assert_eq!(
            decode_into::<24>(model, input, &mut expected),
            batched::<B>(model, input, &mut actual)
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn experimental_batches_match_the_reference() {
        for model in [
            Model::new(&[40000, 25536]).unwrap(),
            Model::new(&[65536]).unwrap(),
        ] {
            for n in [0, 1, 3, 16, 17, 128, 257] {
                let symbols: Vec<_> = (0..n)
                    .map(|i| model.lookup((i * 7111) as u16).symbol)
                    .collect();
                let mut bytes = encode::<24>(&model, &symbols).unwrap();
                for cut in 0..=bytes.len() {
                    check::<1>(&model, &bytes[..cut], n);
                    check::<8>(&model, &bytes[..cut], n);
                    check::<32>(&model, &bytes[..cut], n);
                    check::<128>(&model, &bytes[..cut], n);
                }
                bytes.push(0xff);
                check::<8>(&model, &bytes, n);
            }
        }
    }
}
