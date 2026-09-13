use delayed_coding::{
    encode_branches_into, Branch, Decoder, Error, Interval, Layout, Model, Workspace,
};

fn import(model: &Model) -> Vec<Option<Branch>> {
    let mut intervals: Vec<Vec<Interval>> = vec![Vec::new(); model.alphabet_size()];
    for word in 0..65536u32 {
        let symbol = model.lookup(word as u16).symbol as usize;
        if let Some(last) = intervals[symbol].last_mut().filter(|i| i.end == word) {
            last.end += 1;
        } else {
            intervals[symbol].push(Interval {
                start: word,
                end: word + 1,
            });
        }
    }
    intervals
        .iter()
        .map(|i| {
            if i.is_empty() {
                None
            } else {
                Some(Branch::new(i).unwrap())
            }
        })
        .collect()
}

#[test]
fn branch_validation_and_mapping() {
    for intervals in [
        vec![],
        vec![Interval { start: 1, end: 1 }],
        vec![Interval { start: 2, end: 1 }],
        vec![Interval {
            start: 0,
            end: 65537,
        }],
        vec![Interval { start: 0, end: 4 }, Interval { start: 3, end: 5 }],
        vec![
            Interval { start: 10, end: 12 },
            Interval { start: 0, end: 1 },
        ],
    ] {
        assert!(matches!(Branch::new(&intervals), Err(Error::InvalidModel)));
    }
    assert!(Branch::interval(u32::MAX, 2).is_err());
    assert!(Branch::interval(0, 0).is_err());
    for word in 0..=u16::MAX {
        assert_eq!(Branch::raw(word).embed(0), Ok(word));
    }
    for model in [
        Model::new(&[0, 1, 65535]).unwrap(),
        Model::new(&[1000, 20000, 44536]).unwrap(),
        Model::new(&[256; 256]).unwrap(),
    ] {
        let branches = import(&model);
        for word in 0..=u16::MAX {
            let decoded = model.lookup(word);
            let branch = branches[decoded.symbol as usize].as_ref().unwrap();
            assert_eq!(branch.frequency(), decoded.frequency);
            assert_eq!(branch.embed(decoded.remainder), Ok(word));
            assert_eq!(branch.embed(decoded.frequency), Err(Error::InvalidSymbol));
        }
    }
}

enum Read {
    Model(usize),
    Uniform(u32, u32),
    Raw,
}
struct Op {
    branch: Branch,
    read: Read,
    value: u32,
}

fn mixed<const D: u32, const L: usize>() {
    let models = [
        Model::new(&[1, 65535]).unwrap(),
        Model::new(&[40000, 20000, 5536]).unwrap(),
    ];
    let imported = models.each_ref().map(import);
    let mut rng = 123456u64;
    let mut ops = Vec::new();
    let mut previous = 0;
    for i in 0..4097 {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        let op = match i % 5 {
            0 | 1 => {
                let context = previous as usize % 2;
                let value = models[context].lookup(rng as u16).symbol;
                Op {
                    branch: imported[context][value as usize].as_ref().unwrap().clone(),
                    read: Read::Model(context),
                    value,
                }
            }
            2 => {
                let frequency = [32768, 21845, 65536, 997][i % 4];
                let count = 65536 / frequency;
                let value = rng as u32 % count;
                Op {
                    branch: Branch::interval(value * frequency, frequency).unwrap(),
                    read: Read::Uniform(frequency, count),
                    value,
                }
            }
            _ => Op {
                branch: Branch::raw(rng as u16),
                read: Read::Raw,
                value: u32::from(rng as u16),
            },
        };
        previous = op.value;
        ops.push(op);
    }
    let mut storage = vec![0xa5; ops.len() * 2 + 1];
    let mut workspace = Workspace::with_capacity(ops.len());
    let capacity = workspace.capacity();
    let range = encode_branches_into::<D, L>(
        ops.iter().map(|op| &op.branch),
        &mut storage,
        &mut workspace,
    )
    .unwrap();
    assert_eq!(workspace.capacity(), capacity);
    assert!(storage[..range.start].iter().all(|&b| b == 0xa5));
    let payload = &storage[range];
    let mut decoder = Decoder::<D, L>::new(payload).unwrap();
    let mut layout = Layout::<D, L>::new().unwrap();
    previous = 0;
    for op in &ops {
        let source = layout.push_frequency(op.branch.frequency()).unwrap();
        let before = decoder.bytes_read();
        let value = match op.read {
            Read::Model(context) => {
                assert_eq!(context, previous as usize % 2);
                decoder.read(&models[previous as usize % 2]).unwrap()
            }
            Read::Uniform(f, c) => decoder.read_uniform(f, c).unwrap(),
            Read::Raw => u32::from(decoder.read_raw().unwrap()),
        };
        assert_eq!(value, op.value);
        assert_eq!(
            decoder.bytes_read() - before,
            if source.is_some() { 2 } else { 0 }
        );
        previous = value;
    }
    decoder.finish().unwrap();
    assert_eq!(layout.encoded_len(), payload.len());
    let mut scalar = Vec::new();
    for lane in 0..L {
        let branches: Vec<_> = ops
            .iter()
            .skip(lane)
            .step_by(L)
            .map(|op| &op.branch)
            .collect();
        let mut buffer = vec![0; branches.len() * 2];
        let range = encode_branches_into::<D, 1>(
            branches.iter().copied(),
            &mut buffer,
            &mut Workspace::default(),
        )
        .unwrap();
        scalar.push(buffer[range].to_vec());
    }
    let mut cursors = [0usize; L];
    let mut merged = Vec::new();
    let mut layout = Layout::<D, L>::new().unwrap();
    for (i, op) in ops.iter().enumerate() {
        if layout
            .push_frequency(op.branch.frequency())
            .unwrap()
            .is_some()
        {
            let lane = i % L;
            merged.extend_from_slice(&scalar[lane][cursors[lane]..cursors[lane] + 2]);
            cursors[lane] += 2;
        }
    }
    assert_eq!(merged, payload);
    let mut too_small = vec![0xa5; payload.len() - 1];
    assert_eq!(
        encode_branches_into::<D, L>(
            ops.iter().map(|op| &op.branch),
            &mut too_small,
            &mut workspace
        ),
        Err(Error::OutputTooSmall)
    );
    assert!(too_small.iter().all(|&b| b == 0xa5));
}

#[test]
fn mixed_contexts_partitions_raw_and_all_lanes() {
    mixed::<16, 1>();
    mixed::<24, 1>();
    mixed::<32, 1>();
    mixed::<16, 4>();
    mixed::<24, 4>();
    mixed::<32, 4>();
    mixed::<24, 2>();
    mixed::<24, 8>();
}

#[test]
fn partition_tail_validation_is_sticky() {
    let mut decoder = Decoder::<24>::new(&[255, 255]).unwrap();
    assert_eq!(decoder.read_uniform(21845, 3), Err(Error::InvalidState));
    assert_eq!(decoder.read_raw(), Err(Error::InvalidState));
    assert_eq!(decoder.finish(), Err(Error::InvalidState));
    for (f, c) in [(0, 1), (1, 0), (65537, 1), (32768, 3), (1, u32::MAX)] {
        let mut decoder = Decoder::<24>::new(&[0, 0]).unwrap();
        assert_eq!(decoder.read_uniform(f, c), Err(Error::InvalidModel));
        assert_eq!(decoder.bytes_read(), 0);
        assert_eq!(decoder.read_raw(), Err(Error::InvalidModel));
    }
}
