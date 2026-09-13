use delayed_coding::{
    decode_into, decode_lookahead_into, encode, encode_events_interleaved_into, Decoder, Error,
    Event, Layout, Model, TableOptions, Workspace,
};

#[test]
fn allocating_encoder_does_not_retain_worst_case_payload_capacity() {
    let model = Model::new(&[65536]).unwrap();
    let input = vec![0; 10000];
    let payload = encode::<24>(&model, &input).unwrap();
    assert_eq!(payload.len(), 4);
    assert_eq!(payload.capacity(), payload.len());
    let mut output = vec![99; input.len()];
    decode_into::<24>(&model, &payload, &mut output).unwrap();
    assert_eq!(input, output);
}

fn verify<const D: u32, const L: usize>() {
    let frequencies = [1, 127, 4096, 28672, 32640];
    let a = Model::new(&frequencies).unwrap();
    let b = Model::new(&frequencies.into_iter().rev().collect::<Vec<_>>()).unwrap();
    let mut rng = 123456u64;
    let symbols: Vec<_> = (0..2049)
        .map(|_| {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            a.lookup(rng as u16).symbol
        })
        .collect();
    let mut layout = Layout::<D, L>::new().unwrap();
    let positions: Vec<_> = symbols
        .iter()
        .map(|&s| layout.push_frequency(frequencies[s as usize]).unwrap())
        .collect();
    for (model, reversed) in [(&a, false), (&b, true)] {
        let events: Vec<_> = symbols
            .iter()
            .map(|&s| Event {
                model,
                symbol: if reversed { 4 - s } else { s },
            })
            .collect();
        let mut storage = vec![0; layout.encoded_len()];
        let range = encode_events_interleaved_into::<D, L>(
            &events,
            &mut storage,
            &mut Workspace::default(),
        )
        .unwrap();
        assert_eq!(range, 0..layout.encoded_len());
        let mut decoder = Decoder::<D, L>::new(&storage).unwrap();
        for (event, position) in events.iter().zip(&positions) {
            let before = decoder.bytes_read();
            assert_eq!(decoder.read(model).unwrap(), event.symbol);
            if let Some(offset) = position {
                assert_eq!(before, *offset);
                assert_eq!(decoder.bytes_read(), before + 2);
            } else {
                assert_eq!(decoder.bytes_read(), before);
            }
        }
        decoder.finish().unwrap();
        // Appending a suffix never changes the layout of an existing prefix.
        for n in [0, 1, 2, 3, 16, 127, 2049] {
            let mut prefix = Layout::<D, L>::new().unwrap();
            for (event, expected) in events[..n].iter().zip(&positions) {
                let f = model.frequencies()[event.symbol as usize];
                assert_eq!(prefix.push_frequency(f).unwrap(), *expected);
            }
            let mut bytes = vec![0; prefix.encoded_len()];
            let range = encode_events_interleaved_into::<D, L>(
                &events[..n],
                &mut bytes,
                &mut Workspace::default(),
            )
            .unwrap();
            assert_eq!(range, 0..prefix.encoded_len());
        }
    }
}

#[test]
fn layout_is_frequency_only_and_prefix_local() {
    verify::<16, 1>();
    verify::<24, 1>();
    verify::<32, 1>();
    verify::<16, 2>();
    verify::<24, 4>();
    verify::<32, 8>();
}

#[test]
fn layout_rejects_invalid_parameters_without_mutation() {
    assert!(matches!(Layout::<15>::new(), Err(Error::InvalidDelay)));
    assert!(matches!(Layout::<33>::new(), Err(Error::InvalidDelay)));
    assert!(matches!(Layout::<24, 0>::new(), Err(Error::InvalidLanes)));
    assert!(matches!(Layout::<24, 3>::new(), Err(Error::InvalidLanes)));
    let mut layout = Layout::<24>::new().unwrap();
    for f in [0, 65537, u32::MAX] {
        assert_eq!(layout.push_frequency(f), Err(Error::InvalidModel));
        assert_eq!(layout.encoded_len(), 0);
    }
    assert_eq!(layout.push_frequency(65536), Ok(Some(0)));
}

fn compare<const D: u32>(model: &Model, payload: &[u8], count: usize) {
    let mut serial = vec![u32::MAX; count];
    let mut ahead = serial.clone();
    assert_eq!(
        decode_into::<D>(model, payload, &mut serial),
        decode_lookahead_into::<D>(model, payload, &mut ahead)
    );
    assert_eq!(serial, ahead);
}

fn differential<const D: u32>() {
    let mut rng = 0x5eeda11a5u64;
    for weights in [
        vec![65536],
        vec![65535, 1],
        vec![32768, 32768],
        vec![40000, 25536],
        vec![256; 256],
        vec![1; 65536],
    ] {
        for direct in [false, true] {
            let model = Model::new(&weights).unwrap().with_tables(TableOptions {
                direct_encode: false,
                direct_decode: direct,
            });
            for count in [0, 1, 2, 3, 8, 17, 127, 4097] {
                let symbols: Vec<_> = (0..count)
                    .map(|_| {
                        rng ^= rng << 13;
                        rng ^= rng >> 7;
                        rng ^= rng << 17;
                        model.lookup(rng as u16).symbol
                    })
                    .collect();
                let mut bytes = encode::<D>(&model, &symbols).unwrap();
                let mut output = vec![0; count];
                decode_lookahead_into::<D>(&model, &bytes, &mut output).unwrap();
                assert_eq!(symbols, output);
                compare::<D>(&model, &bytes, count);
                for cut in 0..bytes.len().min(64) {
                    compare::<D>(&model, &bytes[..cut], count);
                }
                if !bytes.is_empty() {
                    bytes[0] ^= 0xff;
                }
                compare::<D>(&model, &bytes, count);
                bytes.push(17);
                compare::<D>(&model, &bytes, count);
            }
            for _ in 0..100 {
                let mut bytes = vec![0; (rng % 97) as usize];
                for byte in &mut bytes {
                    rng ^= rng << 13;
                    rng ^= rng >> 7;
                    rng ^= rng << 17;
                    *byte = rng as u8;
                }
                compare::<D>(&model, &bytes, (rng % 129) as usize);
            }
        }
    }
}

#[test]
fn lookahead_matches_serial_including_malformed_inputs() {
    differential::<16>();
    differential::<24>();
    differential::<32>();
    assert_eq!(
        decode_lookahead_into::<15>(&Model::new(&[65536]).unwrap(), &[], &mut []),
        Err(Error::InvalidDelay)
    );
}

fn interleaved_differential<const D: u32, const L: usize>() {
    use delayed_coding::{
        decode_interleaved_into, decode_lookahead_interleaved_into, encode_interleaved_into,
    };
    let model = Model::new(&[1, 127, 4096, 28672, 32640]).unwrap();
    let mut rng = 123456u64;
    for n in [0, 1, 2, 3, 4, 5, 7, 17, 127, 4097] {
        let symbols: Vec<_> = (0..n)
            .map(|_| {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                model.lookup(rng as u16).symbol
            })
            .collect();
        let mut bytes = vec![0; n * 2];
        let range = encode_interleaved_into::<D, L>(
            &model,
            &symbols,
            &mut bytes,
            &mut Workspace::default(),
        )
        .unwrap();
        let payload = &bytes[range];
        let mut output = vec![99; n];
        decode_lookahead_interleaved_into::<D, L>(&model, payload, &mut output).unwrap();
        assert_eq!(symbols, output);
        if L == 4 {
            delayed_coding::decode_grouped4_into::<D>(&model, payload, &mut output).unwrap();
            assert_eq!(symbols, output);
            delayed_coding::decode_branchless4_into::<D>(&model, payload, &mut output).unwrap();
            assert_eq!(symbols, output);
        }
        for cut in 0..payload.len().min(64) {
            let mut a = vec![99; n];
            let mut b = a.clone();
            assert_eq!(
                decode_interleaved_into::<D, L>(&model, &payload[..cut], &mut a),
                decode_lookahead_interleaved_into::<D, L>(&model, &payload[..cut], &mut b)
            );
            assert_eq!(a, b);
            if L == 4 {
                let mut c = vec![99; n];
                let mut d = vec![99; n];
                assert_eq!(
                    decode_interleaved_into::<D, L>(&model, &payload[..cut], &mut c),
                    delayed_coding::decode_grouped4_into::<D>(&model, &payload[..cut], &mut d)
                );
                assert_eq!(c, d);
                d.fill(99);
                assert_eq!(
                    decode_interleaved_into::<D, L>(&model, &payload[..cut], &mut c),
                    delayed_coding::decode_branchless4_into::<D>(&model, &payload[..cut], &mut d)
                );
                assert_eq!(c, d);
            }
        }
        for byte in &mut bytes {
            *byte ^= (rng >> 8) as u8;
        }
        let mut a = vec![99; n];
        let mut b = a.clone();
        assert_eq!(
            decode_interleaved_into::<D, L>(&model, &bytes, &mut a),
            decode_lookahead_interleaved_into::<D, L>(&model, &bytes, &mut b)
        );
        assert_eq!(a, b);
        if L == 4 {
            let mut c = vec![99; n];
            let mut d = vec![99; n];
            assert_eq!(
                decode_interleaved_into::<D, L>(&model, &bytes, &mut c),
                delayed_coding::decode_grouped4_into::<D>(&model, &bytes, &mut d)
            );
            assert_eq!(c, d);
            c.fill(99);
            d.fill(99);
            assert_eq!(
                decode_interleaved_into::<D, L>(&model, &bytes, &mut c),
                delayed_coding::decode_branchless4_into::<D>(&model, &bytes, &mut d)
            );
            assert_eq!(c, d);
        }
    }
}

#[test]
fn interleaved_lookahead_matches_serial() {
    interleaved_differential::<16, 4>();
    interleaved_differential::<24, 4>();
    interleaved_differential::<32, 4>();
    interleaved_differential::<24, 2>();
    interleaved_differential::<24, 8>();
}

fn all_masks<const D: u32>() {
    for direct in [false, true] {
        let model = Model::new(&[1, 4095, 8192, 16384, 36864])
            .unwrap()
            .with_tables(delayed_coding::TableOptions {
                direct_encode: false,
                direct_decode: direct,
            });
        for mask in 0..16 {
            let mut symbols = Vec::new();
            for _ in 0..3 {
                for lane in 0..4 {
                    symbols.push(if mask & (1 << lane) != 0 { 4 } else { 0 });
                }
            }
            // Every lane has crossed/not crossed the threshold according to mask;
            // leave enough physical words to exercise the fast eight-byte window.
            symbols.extend([0; 68]);
            let mut storage = vec![0; symbols.len() * 2];
            let range = delayed_coding::encode_interleaved_into::<D, 4>(
                &model,
                &symbols,
                &mut storage,
                &mut Workspace::default(),
            )
            .unwrap();
            let payload = &storage[range];
            for cut in 0..=payload.len() {
                for n in [0, 1, 3, 4, 5, 12, 15, 16, 17, symbols.len()] {
                    let mut ordinary = vec![u32::MAX; n];
                    let mut optimized = ordinary.clone();
                    assert_eq!(
                        delayed_coding::decode_interleaved_into::<D, 4>(
                            &model,
                            &payload[..cut],
                            &mut ordinary
                        ),
                        delayed_coding::decode_branchless4_into::<D>(
                            &model,
                            &payload[..cut],
                            &mut optimized
                        ),
                        "D={D}, direct={direct}, mask={mask}, cut={cut}, n={n}"
                    );
                    assert_eq!(ordinary, optimized);
                }
            }
        }
    }
}

#[test]
fn branchless_all_source_masks_and_exact_input_tails() {
    all_masks::<16>();
    all_masks::<24>();
    all_masks::<32>();
}
