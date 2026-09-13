use delayed_coding::{
    decode_into, encode, encode_events_into, encode_into, max_encoded_size, Decoder, Error, Event,
    Model, Workspace,
};

struct Random(u64);

fn exact_interleaved<const D: u32, const L: usize>() {
    for weights in [
        vec![65536],
        vec![1, 65535],
        vec![32768, 32768],
        vec![256; 256],
    ] {
        for direct in [false, true] {
            let model = Model::new(&weights)
                .unwrap()
                .with_tables(delayed_coding::TableOptions {
                    direct_encode: direct,
                    direct_decode: false,
                });
            for n in [0, 1, 3, 4, 5, 7, 8, 9, 17, 4097] {
                let symbols: Vec<_> = (0..n)
                    .map(|i| model.lookup((i * 1337) as u16).symbol)
                    .collect();
                let mut workspace = Workspace::default();
                let mut worst = vec![0xa5; n * 2 + 3];
                let range = delayed_coding::encode_interleaved_into::<D, L>(
                    &model,
                    &symbols,
                    &mut worst,
                    &mut workspace,
                )
                .unwrap();
                assert!(worst[..range.start].iter().all(|&b| b == 0xa5));
                let mut exact = vec![0; range.len()];
                let exact_range = delayed_coding::encode_interleaved_into::<D, L>(
                    &model,
                    &symbols,
                    &mut exact,
                    &mut workspace,
                )
                .unwrap();
                assert_eq!(exact_range.start, 0);
                assert_eq!(&worst[range], exact);
                let mut restored = vec![99; n];
                delayed_coding::decode_interleaved_into::<D, L>(&model, &exact, &mut restored)
                    .unwrap();
                assert_eq!(symbols, restored);
            }
        }
    }
}

#[test]
fn interleaved_exact_capacity_and_untouched_prefix() {
    exact_interleaved::<16, 4>();
    exact_interleaved::<24, 4>();
    exact_interleaved::<32, 4>();
    exact_interleaved::<16, 8>();
    exact_interleaved::<24, 8>();
    exact_interleaved::<32, 8>();
}
impl Random {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }
}

fn roundtrip<const D: u32>(model: &Model, input: &[u32]) {
    let payload = encode::<D>(model, input).unwrap();
    let mut output = vec![u32::MAX; input.len()];
    decode_into::<D>(model, &payload, &mut output).unwrap();
    assert_eq!(input, output);
    // No writes beyond the exact payload range, even with odd capacity/alignment.
    let mut storage = vec![0xa5; payload.len() + 3];
    let range = encode_into::<D>(model, input, &mut storage, &mut Workspace::default()).unwrap();
    assert_eq!(&storage[range.clone()], payload);
    assert_eq!(&storage[..range.start], &[0xa5; 3]);
}

fn check_inverse(model: &Model) {
    let mut seen = vec![0u32; model.alphabet_size()];
    for code in 0..=u16::MAX {
        let decoded = model.lookup(code);
        assert!(decoded.remainder < decoded.frequency);
        assert_eq!(
            model.embed(decoded.symbol, decoded.remainder).unwrap(),
            code
        );
        seen[decoded.symbol as usize] += 1;
    }
    assert_eq!(seen, model.frequencies());
}

#[test]
fn model_boundaries_and_all_code_points() {
    for weights in [
        vec![65536],
        vec![0, 65536, 0],
        vec![1, 65535],
        vec![32768, 32768],
        vec![1, 1, 65534],
        vec![1; 65536],
    ] {
        check_inverse(&Model::new(&weights).unwrap());
    }
    for weights in [
        vec![],
        vec![0],
        vec![1],
        vec![65537],
        vec![u32::MAX; 2],
        vec![0; 65537],
    ] {
        assert!(matches!(Model::new(&weights), Err(Error::InvalidModel)));
    }
}

#[test]
fn normalized_counts_are_deterministic_and_preserve_support() {
    for counts in [
        vec![10, 0, 20, 1],
        vec![u32::MAX; 256],
        vec![1; 65536],
        vec![0, 1],
    ] {
        let weights = Model::normalize(&counts).unwrap();
        assert_eq!(weights.iter().map(|&x| u64::from(x)).sum::<u64>(), 65536);
        assert_eq!(weights, Model::normalize(&counts).unwrap());
        for (count, weight) in counts.iter().zip(&weights) {
            assert_eq!(*count == 0, *weight == 0);
        }
        check_inverse(&Model::new(&weights).unwrap());
    }
    assert!(Model::normalize(&[0, 0]).is_err());
    assert!(Model::normalize(&[]).is_err());
    assert_eq!(
        Model::normalize(&[1, 1, 1]).unwrap(),
        vec![21846, 21845, 21845]
    );
}

#[test]
fn short_sequences_exhaustive() {
    for weights in [vec![1, 65535], vec![32768, 32768], vec![40000, 25536]] {
        let model = Model::new(&weights).unwrap();
        for length in 0..=10 {
            for bits in 0..(1usize << length) {
                let input: Vec<u32> = (0..length).map(|i| ((bits >> i) & 1) as u32).collect();
                roundtrip::<16>(&model, &input);
                roundtrip::<24>(&model, &input);
                roundtrip::<32>(&model, &input);
            }
        }
    }
}

#[test]
fn random_models_and_blocks() {
    let mut rng = Random(0x5eeda11a5);
    for iteration in 0..80 {
        let alphabet = 1 + (rng.next() % 511) as usize;
        let mut counts: Vec<u32> = (0..alphabet).map(|_| rng.next() % 1000).collect();
        counts[0] += 1;
        let model = Model::from_counts(&counts).unwrap();
        if iteration < 16 {
            check_inverse(&model);
        }
        let length = if iteration == 0 {
            131073
        } else {
            (rng.next() % 4097) as usize
        };
        let input: Vec<u32> = (0..length)
            .map(|_| model.lookup(rng.next() as u16).symbol)
            .collect();
        roundtrip::<16>(&model, &input);
        roundtrip::<24>(&model, &input);
        roundtrip::<32>(&model, &input);
    }
}

#[test]
fn rare_symbols_and_long_constant_runs() {
    for weights in [vec![65536], vec![65535, 1], vec![1; 65536]] {
        let model = Model::new(&weights).unwrap();
        let mut input = vec![0; 100_003];
        input[100_002] = (weights.len() - 1) as u32;
        roundtrip::<16>(&model, &input);
        roundtrip::<24>(&model, &input);
        roundtrip::<32>(&model, &input);
    }
}

#[test]
fn conditional_model_selection() {
    let models = [
        Model::new(&[65000, 536]).unwrap(),
        Model::new(&[1000, 64536]).unwrap(),
    ];
    let mut rng = Random(1234);
    let mut previous = 0;
    let events: Vec<_> = (0..10001)
        .map(|_| {
            let model = &models[previous];
            let symbol = model.lookup(rng.next() as u16).symbol;
            previous = symbol as usize;
            Event { model, symbol }
        })
        .collect();
    let mut storage = vec![0; max_encoded_size(events.len()).unwrap()];
    let range = encode_events_into::<24>(&events, &mut storage, &mut Workspace::default()).unwrap();
    let mut decoder = Decoder::<24>::new(&storage[range]).unwrap();
    let mut previous = 0;
    for event in &events {
        let symbol = decoder.read(&models[previous]).unwrap();
        assert_eq!(symbol, event.symbol);
        previous = symbol as usize;
    }
    decoder.finish().unwrap();
}

#[test]
fn invalid_symbols_capacity_and_delay() {
    let model = Model::new(&[0, 65536]).unwrap();
    let mut output = [0xa5; 16];
    let mut work = Workspace::default();
    for input in [&[1, 0][..], &[2], &[u32::MAX]] {
        assert_eq!(
            encode_into::<24>(&model, input, &mut output, &mut work),
            Err(Error::InvalidSymbol)
        );
        assert_eq!(output, [0xa5; 16]);
    }
    assert_eq!(
        encode_into::<24>(&model, &[1], &mut output[..1], &mut work),
        Err(Error::OutputTooSmall)
    );
    assert_eq!(output, [0xa5; 16]);
    assert_eq!(encode::<15>(&model, &[]), Err(Error::InvalidDelay));
    assert_eq!(encode::<64>(&model, &[]), Err(Error::InvalidDelay));
    assert!(matches!(Decoder::<33>::new(&[]), Err(Error::InvalidDelay)));
    assert_eq!(max_encoded_size(usize::MAX), Err(Error::InputTooLarge));
    assert_eq!(model.embed(0, 0), Err(Error::InvalidSymbol));
    assert_eq!(model.embed(1, 65536), Err(Error::InvalidSymbol));
}

#[test]
fn truncation_trailing_bytes_and_sticky_errors() {
    let model = Model::new(&[1; 256].map(|x| x * 256)).unwrap();
    let input: Vec<u32> = (0..100).collect();
    let payload = encode::<24>(&model, &input).unwrap();
    for cut in 0..payload.len() {
        assert!(decode_into::<24>(&model, &payload[..cut], &mut [0; 100]).is_err());
    }
    let mut extra = payload.clone();
    extra.push(0);
    assert_eq!(
        decode_into::<24>(&model, &extra, &mut [0; 100]),
        Err(Error::TrailingInput)
    );
    let mut decoder = Decoder::<24>::new(&[]).unwrap();
    assert_eq!(decoder.read(&model), Err(Error::TruncatedInput));
    assert_eq!(decoder.read(&model), Err(Error::TruncatedInput));
    assert_eq!(decoder.finish(), Err(Error::TruncatedInput));
}

#[test]
fn malformed_payloads_never_panic() {
    let mut rng = Random(0x123456789);
    for counts in [&[1, 65535][..], &[1, 1, 1], &[0, 1]] {
        let model = Model::from_counts(counts).unwrap();
        for _ in 0..1000 {
            let bytes: Vec<u8> = (0..rng.next() % 64).map(|_| rng.next() as u8).collect();
            let count = (rng.next() % 1024) as usize;
            let mut output = vec![0; count];
            let _ = decode_into::<16>(&model, &bytes, &mut output);
            let _ = decode_into::<24>(&model, &bytes, &mut output);
            let _ = decode_into::<32>(&model, &bytes, &mut output);
        }
    }
}

#[test]
fn models_can_be_shared_across_threads() {
    let model = Model::new(&[32768, 32768]).unwrap();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| roundtrip::<24>(&model, &[0, 1, 1, 0]));
        }
    });
}

#[test]
fn optional_tables_preserve_all_mappings_and_payloads() {
    use delayed_coding::TableOptions;
    let model = Model::new(&[1, 32768, 30000, 2767, 0]).unwrap();
    let input: Vec<_> = (0..10001).map(|i| model.lookup(i as u16).symbol).collect();
    let expected = encode::<24>(&model, &input).unwrap();
    for flags in 0..4 {
        let prepared = model.clone().with_tables(TableOptions {
            direct_encode: flags & 1 != 0,
            direct_decode: flags & 2 != 0,
        });
        check_inverse(&prepared);
        assert_eq!(encode::<24>(&prepared, &input).unwrap(), expected);
        roundtrip::<16>(&prepared, &input);
        roundtrip::<24>(&prepared, &input);
        roundtrip::<32>(&prepared, &input);
    }
}

fn interleaved<const D: u32, const L: usize>() {
    use delayed_coding::{decode_interleaved_into, encode_interleaved_into, TableOptions};
    let mut rng = Random(555123);
    for flags in 0..4 {
        let model = Model::new(&[1, 65500, 35])
            .unwrap()
            .with_tables(TableOptions {
                direct_encode: flags & 1 != 0,
                direct_decode: flags & 2 != 0,
            });
        for length in (0..33).chain([1001, 65537]) {
            let input: Vec<_> = (0..length).map(|_| rng.next() % 3).collect();
            let lanes: Vec<_> = (0..L)
                .map(|lane| {
                    let symbols: Vec<_> = input.iter().skip(lane).step_by(L).copied().collect();
                    encode::<D>(&model, &symbols).unwrap()
                })
                .collect();
            // Independent oracle: merge physical words from separately encoded
            // scalar streams, using the forward probability schedule only.
            let mut positions = [0; L];
            let mut denominators = [1u64; L];
            let mut virtuals = [false; L];
            let frequencies = model.frequencies();
            let mut expected = Vec::new();
            for (i, &symbol) in input.iter().enumerate() {
                let lane = i % L;
                if !virtuals[lane] {
                    expected.extend_from_slice(&lanes[lane][positions[lane]..positions[lane] + 2]);
                    positions[lane] += 2;
                }
                denominators[lane] *= u64::from(frequencies[symbol as usize]);
                virtuals[lane] = denominators[lane] >= (1u64 << D);
                if virtuals[lane] {
                    denominators[lane] >>= 16;
                }
            }
            let mut bytes = vec![0; input.len() * 2];
            let range = encode_interleaved_into::<D, L>(
                &model,
                &input,
                &mut bytes,
                &mut Workspace::default(),
            )
            .unwrap();
            assert_eq!(&bytes[range.clone()], expected);
            let mut output = vec![0; input.len()];
            decode_interleaved_into::<D, L>(&model, &bytes[range.clone()], &mut output).unwrap();
            assert_eq!(output, input);
            if !range.is_empty() {
                assert!(decode_interleaved_into::<D, L>(
                    &model,
                    &bytes[range.start..range.end - 1],
                    &mut output
                )
                .is_err());
            }
        }
    }
}

#[test]
fn interleaving_matches_independent_scalar_streams() {
    interleaved::<16, 1>();
    interleaved::<16, 2>();
    interleaved::<16, 4>();
    interleaved::<16, 8>();
    interleaved::<24, 1>();
    interleaved::<24, 2>();
    interleaved::<24, 4>();
    interleaved::<24, 8>();
    interleaved::<32, 1>();
    interleaved::<32, 2>();
    interleaved::<32, 4>();
    interleaved::<32, 8>();
    assert!(matches!(
        Decoder::<24, 0>::new(&[]),
        Err(Error::InvalidLanes)
    ));
    assert!(matches!(
        Decoder::<24, 3>::new(&[]),
        Err(Error::InvalidLanes)
    ));
}
