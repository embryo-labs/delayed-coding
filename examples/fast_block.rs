//! Fixed-model four-state speed path. Setup is deliberately outside the hot API.
//! cargo run --release --features speculative-encode --example fast_block -- FILE
use delayed_coding::{
    decode_branchless4_into, encode_interleaved_into, Model, TableOptions, Workspace,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = if let Some(path) = std::env::args().nth(1) {
        if std::fs::metadata(&path)?.len() > 64 * 1024 * 1024 {
            return Err("example file must be at most 64 MiB".into());
        }
        std::fs::read(path)?
    } else {
        b"Delayed Coding: reusable models, buffers, and four independent states.\n".repeat(1024)
    };
    if bytes.is_empty() {
        return Err("example requires nonempty input to build a model".into());
    }
    let mut counts = [0u32; 256];
    for &byte in &bytes {
        counts[usize::from(byte)] += 1;
    }
    let model = Model::from_counts(&counts)?.with_tables(TableOptions {
        direct_encode: true,
        direct_decode: true,
    });
    let input: Vec<_> = bytes.iter().map(|&byte| u32::from(byte)).collect();
    let mut encoded = vec![0; input.len() * 2];
    let mut restored = vec![0; input.len()];
    let mut workspace = Workspace::with_capacity(input.len());

    // Reuse all of these allocations across blocks. Model/count/delay/lanes
    // belong in external framing; this example produces only the entropy payload.
    let range = encode_interleaved_into::<24, 4>(&model, &input, &mut encoded, &mut workspace)?;
    decode_branchless4_into::<24>(&model, &encoded[range.clone()], &mut restored)?;
    assert_eq!(input, restored);
    println!(
        "{} symbols -> {} payload bytes; model tables {} bytes; speculative encode: {}",
        input.len(),
        range.len(),
        model.table_bytes(),
        cfg!(feature = "speculative-encode")
    );
    Ok(())
}
