#![no_main]
use delayed_coding::{
    decode_interleaved_into, decode_lookahead_interleaved_into, encode_interleaved_into, Model,
    TableOptions, Workspace,
};
use libfuzzer_sys::fuzz_target;

fn exercise<const D: u32, const L: usize>(model: &Model, payload: &[u8], output_size: usize) {
    let mut output = vec![0; output_size];
    let result = decode_interleaved_into::<D, L>(model, payload, &mut output);
    let mut ahead = vec![0; output_size];
    assert_eq!(
        result,
        decode_lookahead_interleaved_into::<D, L>(model, payload, &mut ahead)
    );
    assert_eq!(output, ahead);
    if L == 4 {
        let mut grouped = vec![0; output_size];
        assert_eq!(
            result,
            delayed_coding::decode_grouped4_into::<D>(model, payload, &mut grouped)
        );
        assert_eq!(output, grouped);
        grouped.fill(0);
        assert_eq!(
            result,
            delayed_coding::decode_branchless4_into::<D>(model, payload, &mut grouped)
        );
        assert_eq!(output, grouped);
    }
    let input: Vec<_> = payload
        .iter()
        .map(|&b| u32::from(b) % model.alphabet_size() as u32)
        .collect();
    let mut storage = vec![0xa5; 2 * input.len() + 1];
    let range =
        encode_interleaved_into::<D, L>(model, &input, &mut storage, &mut Workspace::default())
            .unwrap();
    assert!(storage[..range.start].iter().all(|&b| b == 0xa5));
    let mut restored = vec![0; input.len()];
    decode_interleaved_into::<D, L>(model, &storage[range.clone()], &mut restored).unwrap();
    assert_eq!(input, restored);
    decode_lookahead_interleaved_into::<D, L>(model, &storage[range], &mut restored).unwrap();
    assert_eq!(input, restored);
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 || data.len() > 4096 {
        return;
    }
    let alphabet = usize::from(data[0] % 32) + 1;
    let mut counts = vec![1; alphabet];
    for (count, &byte) in counts.iter_mut().zip(data[4..].iter()) {
        *count += u32::from(byte);
    }
    let model = Model::from_counts(&counts)
        .unwrap()
        .with_tables(TableOptions {
            direct_encode: data[0] & 128 != 0,
            direct_decode: data[0] & 64 != 0,
        });
    let output_size = usize::from(data[3]) * 16;
    let payload = &data[4..];
    match (data[1] % 3, data[2] % 2) {
        (0, 0) => exercise::<16, 1>(&model, payload, output_size),
        (0, 1) => exercise::<16, 4>(&model, payload, output_size),
        (1, 0) => exercise::<24, 1>(&model, payload, output_size),
        (1, 1) => exercise::<24, 4>(&model, payload, output_size),
        (2, 0) => exercise::<32, 1>(&model, payload, output_size),
        _ => exercise::<32, 4>(&model, payload, output_size),
    }
});
