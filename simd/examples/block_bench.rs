//! DC-only fixed-model kernel measurement. Setup and I/O are excluded.
use delayed_coding::Model;
use delayed_coding_simd::{EncodeWorkspace, SimdModel};
use std::{hint::black_box, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 1 {
        return Err("usage: block_bench BYTE_FILE".into());
    }
    let bytes = std::fs::read(&args[0])?;
    if bytes.is_empty() || bytes.len() > 64 << 20 {
        return Err("expected 1..64 MiB input".into());
    }
    let mut counts = [0u32; 256];
    for &b in &bytes {
        counts[b as usize] += 1;
    }
    let frequencies = Model::normalize(&counts)?;
    let input: Vec<_> = bytes.iter().map(|&b| u32::from(b)).collect();
    let model = SimdModel::new_cumulative(&frequencies)?;
    let scalar = SimdModel::new_cumulative_scalar(&frequencies)?;
    let mut workspace = EncodeWorkspace::default();
    let mut storage = vec![0; input.len() * 2];
    let range = model.encode64_into(&input, &mut storage, &mut workspace)?;
    let mut out = vec![0; input.len()];
    scalar.decode64_bounded(&storage[range.clone()], &mut out)?;
    assert_eq!(out, input);
    model.decode64_bounded(&storage[range.clone()], &mut out)?;
    assert_eq!(out, input);
    let iterations = (16_000_000 / input.len()).max(1);
    println!("backend,symbols,precision,states,payload_bytes,model_heap_bytes,sample,iterations,encode_ns_symbol,decode_ns_symbol");
    for sample in 0..7 {
        let encode_start = Instant::now();
        for _ in 0..iterations {
            black_box(model.encode64_into(
                black_box(&input),
                black_box(&mut storage),
                &mut workspace,
            )?);
        }
        let encode = encode_start.elapsed().as_secs_f64() * 1e9 / (iterations * input.len()) as f64;
        let decode_start = Instant::now();
        for _ in 0..iterations {
            model.decode64_bounded(black_box(&storage[range.clone()]), black_box(&mut out))?;
            black_box(&out);
        }
        let decode = decode_start.elapsed().as_secs_f64() * 1e9 / (iterations * input.len()) as f64;
        println!(
            "{},{},16,64,{},{},{},{},{:.6},{:.6}",
            model.backend(),
            input.len(),
            range.len(),
            model.memory_bytes(),
            sample,
            iterations,
            encode,
            decode
        );
    }
    Ok(())
}
