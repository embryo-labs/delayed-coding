use dc_avx512_prototype::{EncodeWorkspace, SimdModel};
use delayed_coding::{
    decode_interleaved_into, encode_interleaved_into, LogModel, Model, Workspace,
};
use std::io::{BufRead, Write};
use std::{hint::black_box, time::Instant};

fn weights(input: &[u32], precision: u32) -> Vec<u32> {
    let mut counts = [0u64; 256];
    for &id in input {
        counts[id as usize] += 1;
    }
    let total = 1u32 << precision;
    let remaining = total - counts.iter().filter(|&&c| c != 0).count() as u32;
    let mut frequencies = vec![0u32; 256];
    let mut remainders = Vec::new();
    for (id, &count) in counts.iter().enumerate() {
        if count != 0 {
            let scaled = count * u64::from(remaining);
            frequencies[id] = 1 + (scaled / input.len() as u64) as u32;
            remainders.push((scaled % input.len() as u64, id));
        }
    }
    remainders.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let missing = total - frequencies.iter().sum::<u32>();
    for &(_, id) in remainders.iter().take(missing as usize) {
        frequencies[id] += 1;
    }
    frequencies.iter_mut().for_each(|f| *f <<= 16 - precision);
    frequencies
}

fn measure(mut f: impl FnMut(), symbols: usize) -> f64 {
    let minimum: usize = std::env::var("DC_BENCH_MIN_SYMBOLS")
        .map(|s| s.parse().unwrap())
        .unwrap_or(16777216);
    let repeats = (minimum / symbols).max(1);
    for _ in 0..3 {
        f();
    }
    // Optional perf-stat FIFO control excludes setup, model construction and
    // correctness oracles from hardware counters. One filtered codec per run.
    let mut control = std::env::var("DC_PERF_CTL").ok().map(|path| {
        assert!(
            std::env::var("DC_BENCH_CODEC").is_ok(),
            "perf control requires one codec filter"
        );
        let mut command = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        let mut ack = std::io::BufReader::new(
            std::fs::File::open(std::env::var("DC_PERF_ACK").unwrap()).unwrap(),
        );
        command.write_all(b"enable\n").unwrap();
        command.flush().unwrap();
        let mut line = String::new();
        ack.read_line(&mut line).unwrap();
        assert_eq!(
            line.trim_matches(|c: char| c == '\0' || c.is_whitespace()),
            "ack"
        );
        (command, ack)
    });
    let mut samples = Vec::new();
    for _ in 0..7 {
        let start = Instant::now();
        for _ in 0..repeats {
            f();
        }
        samples.push(start.elapsed().as_nanos() as f64 / (repeats * symbols) as f64);
    }
    if let Some((command, ack)) = &mut control {
        command.write_all(b"disable\n").unwrap();
        command.flush().unwrap();
        let mut line = String::new();
        ack.read_line(&mut line).unwrap();
        assert_eq!(
            line.trim_matches(|c: char| c == '\0' || c.is_whitespace()),
            "ack"
        );
        eprintln!("perf_measured_symbols={}", 7 * repeats * symbols);
    }
    samples.sort_by(f64::total_cmp);
    samples[3]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: dc-avx512-prototype FILE [12|16]")?;
    let precision: u32 = std::env::args().nth(2).unwrap_or("12".into()).parse()?;
    if !matches!(precision, 12 | 16) {
        return Err("precision must be 12 or 16".into());
    }
    let input: Vec<_> = if let Some(name) = path.strip_prefix("synthetic:") {
        let len: usize = std::env::args().nth(3).unwrap_or("65536".into()).parse()?;
        if len == 0 || len > 67108864 {
            return Err("length must be 1..67108864".into());
        }
        let mut f = vec![256; 256];
        match name {
            "uniform256" => {}
            "constant" => {
                f.fill(0);
                f[0] = 65536;
            }
            "near_constant" => {
                f.fill(1);
                f[0] += 65280;
            }
            "skewed" => {
                f.fill(128);
                f[0] += 32768;
            }
            _ => return Err("unknown synthetic distribution".into()),
        }
        let generator = Model::new(&f)?;
        let mut rng = 123456u64;
        (0..len)
            .map(|_| {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                generator.lookup(rng as u16).symbol
            })
            .collect()
    } else {
        let size = std::fs::metadata(&path)?.len();
        if size == 0 || size > 67108864 {
            return Err("file must contain 1..64 MiB".into());
        }
        std::fs::read(path)?.into_iter().map(u32::from).collect()
    };
    if !SimdModel::available() {
        return Err("AVX2 + AVX-512F/BW/VL/VBMI2 required for this benchmark".into());
    }
    let frequencies = weights(&input, precision);
    let model = SimdModel::new(&frequencies)?;
    let bytes = model.encode::<16>(&input)?;
    let mut output = vec![0; input.len()];
    model.decode16(&bytes, &mut output)?;
    assert_eq!(input, output);
    println!(
        "operation,precision,codec,symbols,payload_bytes,kernel_lookup_table_bytes,ns_per_symbol"
    );
    let mut row = |name: &str,
                   bytes: &[u8],
                   table_bytes: usize,
                   decoder: &mut dyn FnMut(&[u8], &mut [u32])| {
        if let Ok(filter) = std::env::var("DC_BENCH_CODEC") {
            if name != filter {
                return;
            }
        }
        decoder(bytes, &mut output);
        assert_eq!(input, output);
        let ns = measure(
            || {
                decoder(black_box(bytes), black_box(&mut output));
                black_box(&output);
            },
            input.len(),
        );
        assert_eq!(input, output);
        println!(
            "decode,{precision},{name},{},{},{table_bytes},{ns:.4}",
            input.len(),
            bytes.len()
        );
    };
    row(
        "dc16_avx512_16",
        &bytes,
        model.decode_table_bytes(),
        &mut |b, o| model.decode16(b, o).unwrap(),
    );
    let bytes32 = model.encode::<32>(&input)?;
    row(
        "dc16_avx512_32",
        &bytes32,
        model.decode_table_bytes(),
        &mut |b, o| model.decode32(b, o).unwrap(),
    );
    let bytes64 = model.encode::<64>(&input)?;
    row(
        "dc16_avx512_64",
        &bytes64,
        model.decode_table_bytes(),
        &mut |b, o| model.decode64(b, o).unwrap(),
    );
    let cumulative = SimdModel::new_cumulative(&frequencies)?;
    let mut storage = vec![0; input.len() * 2];
    let mut workspace = EncodeWorkspace::default();
    let range = cumulative.encode64_into(&input, &mut storage, &mut workspace)?;
    let oracle = cumulative.encode::<64>(&input)?;
    assert_eq!(oracle, storage[range.clone()]);
    row(
        "dc16_cumulative_avx512_64",
        &storage[range.clone()],
        cumulative.decode_table_bytes(),
        &mut |b, o| cumulative.decode64(b, o).unwrap(),
    );
    row(
        "dc16_cumulative_bounded_64",
        &storage[range.clone()],
        cumulative.decode_table_bytes(),
        &mut |b, o| cumulative.decode64_bounded(b, o).unwrap(),
    );
    if std::env::var("DC_BENCH_CODEC").map_or(true, |s| s == "dc16_cumulative_encode_64") {
        let ns = measure(
            || {
                black_box(
                    cumulative
                        .encode64_into(
                            black_box(&input),
                            black_box(&mut storage),
                            black_box(&mut workspace),
                        )
                        .unwrap(),
                );
            },
            input.len(),
        );
        println!(
            "encode,{precision},dc16_cumulative_encode_64,{},{},3072,{ns:.4}",
            input.len(),
            range.len()
        );
    }
    let delay24 = cumulative.encode_delay::<24, 64>(&input)?;
    eprintln!(
        "rate_control_precision={precision},lanes=64,delay16_bytes={},delay24_bytes={}",
        range.len(),
        delay24.len()
    );
    row(
        "dc16_scalar_oracle_16",
        &bytes,
        model.scalar_table_bytes(),
        &mut |b, o| model.decode_scalar::<16>(b, o).unwrap(),
    );
    let ordinary = Model::new(&frequencies)?;
    let mut storage = vec![0; input.len() * 2];
    let mut workspace = Workspace::with_capacity(input.len());
    let range = encode_interleaved_into::<24, 8>(&ordinary, &input, &mut storage, &mut workspace)?;
    row(
        "dc24_scalar_8",
        &storage[range],
        ordinary.table_bytes(),
        &mut |b, o| decode_interleaved_into::<24, 8>(&ordinary, b, o).unwrap(),
    );
    let log = LogModel::new(&frequencies)?;
    let range = log.encode_into::<4>(&input, &mut storage, &mut workspace)?;
    row(
        "log24_scalar_4",
        &storage[range],
        log.table_bytes(),
        &mut |b, o| log.decode_into::<4>(b, o).unwrap(),
    );
    if let Ok(filter) = std::env::var("DC_BENCH_CODEC") {
        if filter == "store_floor" {
            let ns = measure(
                || {
                    black_box(&mut output).fill(0);
                    black_box(&output);
                },
                input.len(),
            );
            eprintln!("store_floor_ns_per_symbol={ns:.4}");
        } else if filter == "copy_floor" {
            let ns = measure(
                || {
                    black_box(&mut output).copy_from_slice(black_box(&input));
                    black_box(&output);
                },
                input.len(),
            );
            eprintln!("copy_floor_ns_per_symbol={ns:.4}");
        }
    }
    Ok(())
}
