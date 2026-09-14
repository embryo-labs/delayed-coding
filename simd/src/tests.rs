use super::*;
use delayed_coding::{encode_interleaved_into, Workspace};

#[test]
fn tails_models_and_scalar_oracles() {
    let models = [
        vec![256; 256],
        vec![4096; 16],
        vec![65536],
        (0..256).map(|i| 2 * i + 1).collect(),
        {
            let mut f = vec![16; 256];
            f[0] += 61440;
            f
        },
        {
            let mut f = vec![1; 256];
            f[0] += 65280;
            f
        },
        vec![0, 32768, 0, 32768],
    ];
    let mut rng = 77u64;
    for frequencies in models {
        let model = SimdModel::new(&frequencies).unwrap();
        for len in (0..80).chain([255, 256, 257, 4095, 4096, 4097, 65536]) {
            let input: Vec<_> = (0..len)
                .map(|_| {
                    rng ^= rng << 13;
                    rng ^= rng >> 7;
                    rng ^= rng << 17;
                    model.model.lookup(rng as u16).symbol
                })
                .collect();
            let encoded = model.encode::<16>(&input).unwrap();
            let mut scalar = vec![0; len];
            let mut simd = vec![0; len];
            model.decode_scalar::<16>(&encoded, &mut scalar).unwrap();
            model.decode16(&encoded, &mut simd).unwrap();
            assert_eq!(input, scalar);
            assert_eq!(input, simd);
            let bytes32 = model.encode::<32>(&input).unwrap();
            model.decode32(&bytes32, &mut simd).unwrap();
            assert_eq!(input, simd);
            let bytes64 = model.encode::<64>(&input).unwrap();
            model.decode64(&bytes64, &mut simd).unwrap();
            assert_eq!(input, simd);
            model.decode64_bounded(&bytes64, &mut simd).unwrap();
            assert_eq!(input, simd);
            // Cross-check the independent encoder against supported DC at L=8.
            let oracle = model.encode::<8>(&input).unwrap();
            let mut bytes = vec![0; len * 2];
            let range = encode_interleaved_into::<16, 8>(
                &model.model,
                &input,
                &mut bytes,
                &mut Workspace::with_capacity(len),
            )
            .unwrap();
            assert_eq!(oracle, bytes[range]);
            // No caller padding: exact-size and deliberately unaligned slices.
            let mut offset = vec![99];
            offset.extend_from_slice(&encoded);
            model.decode16(&offset[1..], &mut simd).unwrap();
            assert_eq!(input, simd);
            if !encoded.is_empty() {
                assert!(model
                    .decode16(&encoded[..encoded.len() - 1], &mut simd)
                    .is_err());
            }
            offset.push(17);
            assert!(model.decode16(&offset[1..], &mut simd).is_err());
        }
    }
}

#[test]
fn malformed_buffers_agree_with_scalar() {
    let mut f = vec![1; 256];
    f[0] += 65280;
    for frequencies in [
        f,
        vec![16; 4096].chunks(16).map(|x| x.iter().sum()).collect(),
    ] {
        let model = SimdModel::new(&frequencies).unwrap();
        let mut rng = 19u64;
        for n in 0..3000 {
            let bytes: Vec<_> = (0..n % 128)
                .map(|_| {
                    rng ^= rng << 13;
                    rng ^= rng >> 7;
                    rng ^= rng << 17;
                    rng as u8
                })
                .collect();
            let mut scalar = vec![0; n % 193];
            let mut simd = scalar.clone();
            let a = model.decode_scalar::<16>(&bytes, &mut scalar);
            let b = model.decode16(&bytes, &mut simd);
            assert_eq!(a.is_ok(), b.is_ok());
            if a.is_ok() {
                assert_eq!(scalar, simd);
            }
            let a = model.decode_scalar::<64>(&bytes, &mut scalar);
            let b = model.decode64_bounded(&bytes, &mut simd);
            assert_eq!(a.is_ok(), b.is_ok());
            if a.is_ok() {
                assert_eq!(scalar, simd);
            }
        }
    }
}

#[test]
fn cumulative_encoder_exact_buffers_and_tails() {
    let mut rng = 351u64;
    let mut frequencies = vec![1; 256];
    frequencies[0] += 65280;
    for f in [
        frequencies,
        vec![256; 256],
        vec![65536],
        vec![0, 32768, 0, 32768],
        (0..256).map(|i| 2 * i + 1).collect(),
    ] {
        let model = SimdModel::new_cumulative(&f).unwrap();
        let mut workspace = EncodeWorkspace::default();
        for len in (0..140).chain([255, 256, 257, 4095, 4096, 4097, 65536]) {
            let input: Vec<_> = (0..len)
                .map(|_| {
                    rng ^= rng << 13;
                    rng ^= rng >> 7;
                    rng ^= rng << 17;
                    model.lookup(rng as u16).symbol
                })
                .collect();
            let oracle = model.encode::<64>(&input).unwrap();
            let mut output = vec![0xa5; oracle.len() + 3];
            let range = model
                .encode64_into(&input, &mut output, &mut workspace)
                .unwrap();
            assert_eq!(range, 3..output.len());
            assert_eq!(&output[..3], &[0xa5; 3]);
            assert_eq!(oracle, output[range]);
            let mut restored = vec![0; len];
            model.decode64(&oracle, &mut restored).unwrap();
            assert_eq!(input, restored);
            model.decode64_bounded(&oracle, &mut restored).unwrap();
            assert_eq!(input, restored);
            model.decode_scalar::<64>(&oracle, &mut restored).unwrap();
            assert_eq!(input, restored);
            if !oracle.is_empty() {
                let mut short = vec![0xa5; oracle.len() - 1];
                assert_eq!(
                    model.encode64_into(&input, &mut short, &mut workspace),
                    Err(Error::OutputTooSmall)
                );
                assert!(short.iter().all(|&b| b == 0xa5));
            }
        }
        let valid = f.iter().position(|&x| x != 0).unwrap() as u32;
        for bad_position in [0, 15, 16, 63, 64, 129] {
            let mut input = vec![valid; 130];
            input[bad_position] = u32::MAX;
            let mut output = vec![0xa5; 260];
            assert_eq!(
                model.encode64_into(&input, &mut output, &mut workspace),
                Err(Error::InvalidSymbol)
            );
            assert!(output.iter().all(|&b| b == 0xa5));
        }
    }
}

#[test]
fn forced_scalar_is_byte_identical_and_has_no_direct_tables() {
    for f in [
        vec![256; 256],
        vec![1, 65535],
        vec![0, 65536],
        vec![32768, 0, 32768],
    ] {
        let automatic = SimdModel::new_cumulative(&f).unwrap();
        let scalar = SimdModel::new_cumulative_scalar(&f).unwrap();
        assert_eq!(scalar.backend(), "scalar");
        assert_eq!(scalar.decode_table_bytes(), 0);
        let mut workspace = EncodeWorkspace::default();
        for len in (0..140).chain([4095, 4096, 4097]) {
            let symbols: Vec<_> = (0..len)
                .map(|i| scalar.lookup((i * 7919) as u16).symbol)
                .collect();
            let mut a = vec![0; len * 2];
            let mut b = a.clone();
            let ra = automatic
                .encode64_into(&symbols, &mut a, &mut workspace)
                .unwrap();
            let rb = scalar
                .encode64_into(&symbols, &mut b, &mut workspace)
                .unwrap();
            assert_eq!(a[ra.clone()], b[rb]);
            let mut out = vec![0; len];
            scalar.decode64_bounded(&a[ra.clone()], &mut out).unwrap();
            assert_eq!(out, symbols);
            automatic
                .decode64_bounded(&a[ra.clone()], &mut out)
                .unwrap();
            assert_eq!(out, symbols);
            for size in 0..ra.len().min(80) {
                assert!(automatic
                    .decode64_bounded(&a[ra.start..ra.start + size], &mut out)
                    .is_err());
            }
            if !ra.is_empty() {
                a[ra.start] ^= 0x55;
                let x = scalar.decode64_bounded(&a[ra.clone()], &mut out);
                let mut other = vec![0; len];
                let y = automatic.decode64_bounded(&a[ra], &mut other);
                assert_eq!(x.is_ok(), y.is_ok());
                if x.is_ok() {
                    assert_eq!(out, other);
                }
            }
        }
    }
}
