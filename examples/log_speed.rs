use delayed_coding::{LogModel, Model, Workspace};
use std::{hint::black_box, time::Instant};
fn run<const L: usize>(model: &LogModel, input: &[u32]) {
    let mut bytes = vec![0; input.len() * 2];
    let mut out = vec![0; input.len()];
    let mut workspace = Workspace::with_capacity(input.len());
    let range = model
        .encode_into::<L>(input, &mut bytes, &mut workspace)
        .unwrap();
    model
        .decode_into::<L>(&bytes[range.clone()], &mut out)
        .unwrap();
    assert_eq!(input, out);
    let repeats = (16777216 / input.len()).max(1);
    let mut enc = Vec::new();
    let mut dec = Vec::new();
    for _ in 0..7 {
        let start = Instant::now();
        for _ in 0..repeats {
            black_box(
                model
                    .encode_into::<L>(
                        black_box(input),
                        black_box(&mut bytes),
                        black_box(&mut workspace),
                    )
                    .unwrap(),
            );
        }
        enc.push(start.elapsed().as_nanos() as f64 / (repeats * input.len()) as f64);
        let start = Instant::now();
        for _ in 0..repeats {
            model
                .decode_into::<L>(black_box(&bytes[range.clone()]), black_box(&mut out))
                .unwrap();
            black_box(&out);
        }
        dec.push(start.elapsed().as_nanos() as f64 / (repeats * input.len()) as f64);
    }
    enc.sort_by(f64::total_cmp);
    dec.sort_by(f64::total_cmp);
    println!(
        "log_dc,{L},{},{},{:.4},{:.4}",
        input.len(),
        range.len(),
        enc[3],
        dec[3]
    );
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = if let Some(path) = std::env::args().nth(1) {
        if std::fs::metadata(&path)?.len() > 64 * 1024 * 1024 {
            return Err("file must be <=64 MiB".into());
        }
        std::fs::read(path)?
    } else {
        b"Logarithmic Delayed Coding: fixed models and reusable buffers.\n".repeat(1024)
    };
    if bytes.is_empty() {
        return Err("input must be nonempty".into());
    }
    let mut counts = [0u32; 256];
    for &b in &bytes {
        counts[b as usize] += 1;
    }
    let frequencies = Model::normalize(&counts).unwrap();
    let model = LogModel::new(&frequencies).unwrap();
    eprintln!(
        "experimental separate format; model tables {} bytes",
        model.table_bytes()
    );
    println!("codec,lanes,symbols,payload_bytes,encode_ns_per_symbol,decode_ns_per_symbol");
    let input: Vec<_> = bytes.iter().map(|&b| u32::from(b)).collect();
    run::<4>(&model, &input);
    run::<8>(&model, &input);
    Ok(())
}
