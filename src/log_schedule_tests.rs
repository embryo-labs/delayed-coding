use super::*;
use crate::Model;
#[test]
fn integer_logs_are_conservative() {
    for f in 1..=65536 {
        let exact = f64::from(f).log2() * 256.0;
        let lower = f64::from(log_frequency(f));
        assert!(lower <= exact, "f={f}, lower={lower}, exact={exact}");
        assert!(exact - lower < 1.001);
    }
}

fn verify<const L: usize>(frequencies: &[u32], input: &[u32]) {
    let model = LogModel::new(frequencies).unwrap();
    let mut storage = vec![0xab; input.len() * 2 + 3];
    let mut schedule = Workspace::default();
    let range = model
        .encode_into::<L>(input, &mut storage, &mut schedule)
        .unwrap();
    assert!(storage[..range.start].iter().all(|&b| b == 0xab));
    let mut exact = vec![0; range.len()];
    assert_eq!(
        model
            .encode_into::<L>(input, &mut exact, &mut schedule)
            .unwrap(),
        0..range.len()
    );
    assert_eq!(&storage[range.clone()], exact);
    let mut reverse = vec![0; exact.len()];
    model
        .encode_reverse_impl::<L>(
            input,
            &mut reverse,
            &mut Vec::new(),
            |id| model.symbols[id as usize],
            |id| (model.logs.get(id as usize).copied().unwrap_or(u16::MAX), 0),
        )
        .unwrap();
    assert_eq!(exact, reverse);
    let mut out = vec![0; input.len()];
    model.decode_into::<L>(&exact, &mut out).unwrap();
    assert_eq!(input, out);
    // Independent scalar decoder, including lane/schedule/terminal checks.
    let mut budgets = [0u32; L];
    let mut states = [0u64; L];
    let mut cursor = 0;
    for (i, &expected) in input.iter().enumerate() {
        let lane = i % L;
        let word = if budgets[lane] >= THRESHOLD {
            budgets[lane] -= RENORM;
            let word = states[lane] as u16;
            states[lane] >>= 16;
            word
        } else {
            let word = u16::from_be_bytes([exact[cursor], exact[cursor + 1]]);
            cursor += 2;
            word
        };
        let id = model
            .symbols
            .iter()
            .position(|s| u32::from(word) >= s.start && u32::from(word) - s.start < s.frequency)
            .unwrap();
        assert_eq!(expected as usize, id);
        let symbol = model.symbols[id];
        states[lane] =
            states[lane] * u64::from(symbol.frequency) + u64::from(u32::from(word) - symbol.start);
        budgets[lane] += log_frequency(symbol.frequency);
    }
    assert_eq!(cursor, exact.len());
    assert_eq!(states, [0; L]);
    if !exact.is_empty() {
        assert!(model
            .decode_into::<L>(&exact[..exact.len() - 1], &mut out)
            .is_err());
        exact.push(0);
        assert_eq!(
            model.decode_into::<L>(&exact, &mut out),
            Err(Error::TrailingInput)
        );
    }
}
#[test]
fn roundtrips_and_tails() {
    let mut rng = 123456789u64;
    for weights in [
        vec![65536],
        vec![1, 65535],
        vec![32768, 32768],
        vec![1, 3, 7, 23, 65502],
        vec![256; 256],
    ] {
        let lookup = Model::new(&weights).unwrap();
        for n in [0, 1, 2, 3, 4, 5, 7, 8, 9, 15, 16, 17, 127, 4097, 65537] {
            let input: Vec<_> = (0..n)
                .map(|_| {
                    rng ^= rng << 13;
                    rng ^= rng >> 7;
                    rng ^= rng << 17;
                    lookup.lookup(rng as u16).symbol
                })
                .collect();
            verify::<1>(&weights, &input);
            verify::<2>(&weights, &input);
            verify::<4>(&weights, &input);
            verify::<8>(&weights, &input);
        }
    }
    for _ in 0..16 {
        let counts: Vec<_> = (0..256)
            .map(|_| {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                (rng as u32) & 65535
            })
            .collect();
        let weights = Model::normalize(&counts).unwrap();
        let lookup = Model::new(&weights).unwrap();
        let input: Vec<_> = (0..4097)
            .map(|_| {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                lookup.lookup(rng as u16).symbol
            })
            .collect();
        verify::<4>(&weights, &input);
        verify::<8>(&weights, &input);
    }
}

#[test]
fn wide_alphabets_and_invalid_inputs() {
    verify::<4>(
        &vec![1; 65536],
        &[65535, 0, 32768, 256, 65534, 1, 7, 128, 65535],
    );
    let mut weights = vec![0; 301];
    weights[300] = 65536;
    verify::<8>(&weights, &vec![300; 4097]);
    let model = LogModel::new(&[32768, 0, 32768]).unwrap();
    let mut workspace = Workspace::default();
    for invalid in [1, 3, 255, 256, 65536, u32::MAX] {
        let mut output = [0xab; 16];
        assert_eq!(
            model.encode_into::<4>(&[0, 2, 0, 2, invalid], &mut output, &mut workspace),
            Err(Error::InvalidSymbol)
        );
        assert_eq!(output, [0xab; 16]);
    }
    assert_eq!(
        model.encode_into::<4>(&[0; 4], &mut [0; 7], &mut workspace),
        Err(Error::OutputTooSmall)
    );
    assert_eq!(
        model.encode_into::<0>(&[], &mut [], &mut workspace),
        Err(Error::InvalidLanes)
    );
    assert_eq!(
        model.decode_into::<3>(&[], &mut []),
        Err(Error::InvalidLanes)
    );
    for weights in [vec![], vec![0], vec![65535], vec![65537], vec![u32::MAX, 1]] {
        assert!(matches!(LogModel::new(&weights), Err(Error::InvalidModel)));
    }
}

#[test]
fn malformed_streams_do_not_panic() {
    let mut rng = 0x88776655u64;
    for weights in [vec![65536], vec![1, 65535], vec![1; 65536], vec![256; 256]] {
        let model = LogModel::new(&weights).unwrap();
        for n in 0..64 {
            let payload: Vec<u8> = (0..n)
                .map(|_| {
                    rng ^= rng << 13;
                    rng ^= rng >> 7;
                    rng ^= rng << 17;
                    rng as u8
                })
                .collect();
            for count in [0, 1, 3, 4, 7, 8, 9, 31, 256] {
                let mut out = vec![0; count];
                let _ = model.decode_into::<1>(&payload, &mut out);
                let _ = model.decode_into::<4>(&payload, &mut out);
                let _ = model.decode_into::<8>(&payload, &mut out);
            }
        }
    }
}
