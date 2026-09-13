//! In-memory random-access records with one exactly sized payload arena.
//! cargo run --release --example indexed_records -- [file] [record_width]
//! Demonstrates layout planning; not a serialized/self-describing container.
use delayed_coding::{decode_lookahead_into, encode_into, Layout, Model, Workspace};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let bytes = if let Some(path) = args.first() {
        std::fs::read(path)?
    } else {
        b"independent records; exact allocation; random lookup; shared model".to_vec()
    };
    let width = args
        .get(1)
        .map(|s| s.parse::<usize>())
        .transpose()?
        .unwrap_or(16);
    if bytes.is_empty() || bytes.len() > 1 << 26 || width == 0 || width > 1 << 26 {
        return Err("expected 1..67108864 input bytes and record width".into());
    }
    let mut counts = [0; 256];
    for &byte in &bytes {
        counts[byte as usize] += 1;
    }
    let model = Model::from_counts(&counts)?;
    let frequencies = model.frequencies();
    let symbols: Vec<_> = bytes.iter().map(|&b| u32::from(b)).collect();

    // This pass only uses realized frequencies, not remainder/info state.
    let mut offsets = vec![0u32];
    for record in symbols.chunks(width) {
        let mut layout = Layout::<16>::new()?;
        for &symbol in record {
            layout.push_frequency(frequencies[symbol as usize])?;
        }
        offsets.push(
            offsets
                .last()
                .unwrap()
                .checked_add(u32::try_from(layout.encoded_len())?)
                .ok_or("arena too large")?,
        );
    }
    let mut payload = vec![0; *offsets.last().unwrap() as usize];
    let mut workspace = Workspace::with_capacity(width.min(symbols.len()));
    for (record, bounds) in symbols.chunks(width).zip(offsets.windows(2)) {
        let output = &mut payload[bounds[0] as usize..bounds[1] as usize];
        // encode_into validates/rebuilds its schedule; this example adds a layout
        // pass in exchange for exact arena allocation and preassigned offsets.
        let range = encode_into::<16>(&model, record, output, &mut workspace)?;
        assert_eq!(range, 0..output.len());
    }

    let records = offsets.len() - 1;
    let mut order: Vec<_> = (0..records).collect();
    let mut rng = 123456u64;
    for i in (1..records).rev() {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        order.swap(i, (rng % (i + 1) as u64) as usize);
    }
    let mut output = vec![0; width.min(symbols.len())];
    for id in order {
        let expected = &symbols[id * width..symbols.len().min((id + 1) * width)];
        decode_lookahead_into::<16>(
            &model,
            &payload[offsets[id] as usize..offsets[id + 1] as usize],
            &mut output[..expected.len()],
        )?;
        assert_eq!(&output[..expected.len()], expected);
    }
    println!("Verified {records} independently decoded records in shuffled order.");
    println!(
        "Payload: {} bytes; u32 offset index: {} bytes; total: {} bytes.",
        payload.len(),
        offsets.len() * 4,
        payload.len() + offsets.len() * 4
    );
    println!("Model, original length and record width are external; no checksum or file framing.");
    Ok(())
}
