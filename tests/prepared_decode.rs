use delayed_coding::{
    encode_events_interleaved_into, DecodeModel, Decoder, Event, Model, Workspace,
};

#[test]
fn exact_alias_mapping_and_mixed_records() {
    let mut rng = 1729u64;
    let mut models = vec![
        Model::new(&[65536]).unwrap(),
        Model::new(&[0, 65536, 0]).unwrap(),
    ];
    for n in [2, 3, 7, 16, 69, 256, 1024, 65536] {
        let counts: Vec<_> = (0..n)
            .map(|_| {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                (rng % 100) as u32
            })
            .collect();
        models.push(Model::from_counts(&counts).unwrap());
    }
    let prepared: Vec<_> = models.iter().map(DecodeModel::from_model).collect();
    for (m, p) in models.iter().zip(&prepared) {
        for w in 0..=u16::MAX {
            assert_eq!(m.lookup(w), p.lookup(w));
        }
        let direct = DecodeModel::from_model(m).with_direct_table(m, 16);
        let values: Vec<_> = (0..m.alphabet_size())
            .map(|i| (i as u64).wrapping_mul(0x123456789abcdef))
            .collect();
        let fused = delayed_coding::DecodeValueModel::new(m, &values).unwrap();
        for w in 0..=u16::MAX {
            assert_eq!(m.lookup(w), direct.lookup(w));
            let (decoded, value) = fused.lookup(w);
            assert_eq!(decoded, m.lookup(w));
            assert_eq!(value, values[decoded.symbol as usize]);
        }
    }
    fn run<const LANES: usize>(models: &[Model], prepared: &[DecodeModel]) {
        let events: Vec<_> = (0..2000)
            .map(|i| {
                let model = &models[i % models.len()];
                Event {
                    model,
                    symbol: model.lookup((i * 37) as u16).symbol,
                }
            })
            .collect();
        let mut storage = vec![0; events.len() * 2];
        let mut workspace = Workspace::default();
        let range =
            encode_events_interleaved_into::<16, LANES>(&events, &mut storage, &mut workspace)
                .unwrap();
        let bytes = &storage[range];
        let mut decoder = Decoder::<16, LANES>::new(bytes).unwrap();
        for (i, event) in events.iter().enumerate() {
            assert_eq!(
                decoder
                    .read_prepared(&prepared[i % prepared.len()])
                    .unwrap(),
                event.symbol
            );
        }
        decoder.finish().unwrap();
        let fused: Vec<_> = models
            .iter()
            .map(|m| {
                delayed_coding::DecodeValueModel::new(
                    m,
                    &(0..m.alphabet_size())
                        .map(|i| i as u64 + 17)
                        .collect::<Vec<_>>(),
                )
                .unwrap()
            })
            .collect();
        let mut decoder = Decoder::<16, LANES>::new(bytes).unwrap();
        for (i, event) in events.iter().enumerate() {
            assert_eq!(
                decoder.read_value(&fused[i % fused.len()]).unwrap(),
                u64::from(event.symbol) + 17
            );
        }
        decoder.finish().unwrap();
        for prefix in 0..4 {
            let mut decoder = Decoder::<16, LANES>::new(bytes).unwrap();
            for (i, event) in events.iter().take(prefix).enumerate() {
                assert_eq!(
                    decoder
                        .read_prepared(&prepared[i % prepared.len()])
                        .unwrap(),
                    event.symbol
                );
            }
            let mut i = prefix;
            while i + 4 <= events.len() {
                let m = std::array::from_fn(|j| &prepared[(i + j) % prepared.len()]);
                let actual = decoder.read_prepared4(m).unwrap();
                for j in 0..4 {
                    assert_eq!(actual[j], events[i + j].symbol);
                }
                i += 4;
            }
            while i < events.len() {
                assert_eq!(
                    decoder
                        .read_prepared(&prepared[i % prepared.len()])
                        .unwrap(),
                    events[i].symbol
                );
                i += 1;
            }
            decoder.finish().unwrap();
        }
    }
    run::<1>(&models, &prepared);
    run::<4>(&models, &prepared);
}

#[test]
fn aligned_small_tables_and_precision_validation() {
    for bits in [8, 10, 12, 16] {
        let freq = Model::normalize_precision(&[10000, 0, 123, 1, 2000], bits).unwrap();
        assert_eq!(freq.iter().sum::<u32>(), 65536);
        assert_eq!(freq[1], 0);
        assert!(freq.iter().all(|&f| f % (1 << (16 - bits)) == 0));
        let model = Model::new(&freq).unwrap();
        let prepared = DecodeModel::from_model(&model).with_direct_table(&model, bits);
        assert!(prepared.memory_bytes() <= 8 << bits);
        for w in 0..=u16::MAX {
            assert_eq!(model.lookup(w), prepared.lookup(w));
        }
    }
    assert!(Model::normalize_precision(&[1; 257], 8).is_err());
    assert!(Model::normalize_precision(&[1], 0).is_err());
    assert!(Model::normalize_precision(&[1], 17).is_err());
}

#[test]
fn narrow_record_kernel_matches_streaming_decoder() {
    fn run<const LANES: usize>() {
        let mut rng = 123456u64;
        let models: Vec<_> = (0..155)
            .map(|i| {
                let n = i % 17 + 1;
                let counts: Vec<_> = (0..n)
                    .map(|_| {
                        rng ^= rng << 13;
                        rng ^= rng >> 7;
                        rng ^= rng << 17;
                        (rng % 100 + 1) as u32
                    })
                    .collect();
                Model::new(&Model::normalize_precision(&counts, 10).unwrap()).unwrap()
            })
            .collect();
        let prepared: Vec<_> = models
            .iter()
            .map(|m| DecodeModel::from_model(m).with_direct_table(m, 10))
            .collect();
        for len in 0..=155 {
            let events: Vec<_> = models[..len]
                .iter()
                .map(|model| {
                    rng ^= rng << 13;
                    rng ^= rng >> 7;
                    rng ^= rng << 17;
                    Event {
                        model,
                        symbol: model.lookup(rng as u16).symbol,
                    }
                })
                .collect();
            let mut storage = vec![0; len * 2];
            let mut workspace = Workspace::default();
            let range =
                encode_events_interleaved_into::<16, LANES>(&events, &mut storage, &mut workspace)
                    .unwrap();
            let bytes = &storage[range];
            let mut restored = vec![0; len];
            delayed_coding::decode_prepared_into::<16, LANES>(
                &prepared[..len],
                bytes,
                &mut restored,
            )
            .unwrap();
            assert_eq!(
                restored,
                events.iter().map(|e| e.symbol).collect::<Vec<_>>()
            );
            for size in 0..bytes.len() {
                assert!(delayed_coding::decode_prepared_into::<16, LANES>(
                    &prepared[..len],
                    &bytes[..size],
                    &mut restored
                )
                .is_err());
            }
            // Mutations need not be detectable without a checksum; they must
            // never panic, including debug overflow/bounds checks.
            for i in 0..bytes.len() {
                let mut bad = bytes.to_vec();
                bad[i] ^= 0x5a;
                let _ = delayed_coding::decode_prepared_into::<16, LANES>(
                    &prepared[..len],
                    &bad,
                    &mut restored,
                );
            }
        }
    }
    run::<1>();
    run::<4>();
}
