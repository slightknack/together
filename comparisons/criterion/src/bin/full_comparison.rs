// Full comparison of all RGA implementations against diamond-types
// Uses real editing traces for realistic benchmarks

use std::fs::File;
use std::io::BufReader;
use std::io::Read;
use std::time::Instant;

use flate2::bufread::GzDecoder;
use serde::Deserialize;

use diamond_types::list::ListCRDT;

use together::crdt::rga::RgaBuf;
use together::crdt::rga_trait::Rga;
use together::crdt::yjs::YjsRga;
use together::crdt::diamond::DiamondRga;
use together::crdt::cola::ColaRga;
use together::crdt::json_joy::JsonJoyRga;
use together::crdt::loro::LoroRga;
use together::crdt::rga_optimized::OptimizedRga;
use together::key::KeyPair;
use together::key::KeyPub;

#[derive(Debug, Clone, Deserialize)]
struct TestPatch(usize, usize, String);

#[derive(Debug, Clone, Deserialize)]
struct TestTxn {
    patches: Vec<TestPatch>,
}

#[derive(Debug, Clone, Deserialize)]
struct TestData {
    #[serde(rename = "startContent")]
    start_content: String,
    #[serde(rename = "endContent")]
    end_content: String,
    txns: Vec<TestTxn>,
}

impl TestData {
    fn load(filename: &str) -> TestData {
        let file = File::open(filename).expect("failed to open trace file");
        let mut reader = BufReader::new(file);
        let mut raw_json = Vec::new();

        if filename.ends_with(".gz") {
            let mut decoder = GzDecoder::new(reader);
            decoder.read_to_end(&mut raw_json).expect("failed to decompress");
        } else {
            reader.read_to_end(&mut raw_json).expect("failed to read");
        }

        serde_json::from_slice(&raw_json).expect("failed to parse JSON")
    }
}

fn bench_diamond_types(data: &TestData) -> std::time::Duration {
    let mut doc = ListCRDT::new();
    let agent = doc.get_or_create_agent_id("user");
    
    let start = Instant::now();
    for txn in &data.txns {
        for TestPatch(pos, del, ins) in &txn.patches {
            if *del > 0 {
                doc.delete_without_content(agent, *pos .. *pos + *del);
            }
            if !ins.is_empty() {
                doc.insert(agent, *pos, ins);
            }
        }
    }
    start.elapsed()
}

fn bench_rgabuf(data: &TestData, key_pub: &KeyPub) -> std::time::Duration {
    let mut rga = RgaBuf::new();
    
    let start = Instant::now();
    for txn in &data.txns {
        for TestPatch(pos, del, ins) in &txn.patches {
            if *del > 0 {
                rga.delete(*pos as u64, *del as u64);
            }
            if !ins.is_empty() {
                rga.insert(key_pub, *pos as u64, ins.as_bytes());
            }
        }
    }
    start.elapsed()
}

fn bench_rga_trait<R: Rga<UserId = KeyPub>>(data: &TestData, user: &KeyPub) -> std::time::Duration {
    let mut rga = R::default();
    
    let start = Instant::now();
    for txn in &data.txns {
        for TestPatch(pos, del, ins) in &txn.patches {
            if *del > 0 {
                rga.delete(*pos as u64, *del as u64);
            }
            if !ins.is_empty() {
                rga.insert(user, *pos as u64, ins.as_bytes());
            }
        }
    }
    start.elapsed()
}

struct BenchResult {
    name: &'static str,
    times_ms: [f64; 4],
    ratios: [f64; 4],
}

fn bench_one<R: Rga<UserId = KeyPub>>(name: &str, data: &TestData, user: &KeyPub, dt_ms: f64) -> (f64, f64) {
    eprint!("  {}... ", name);
    let t = bench_rga_trait::<R>(data, user);
    let ms = t.as_secs_f64() * 1000.0;
    let ratio = ms / dt_ms;
    eprintln!("{:.2}ms ({:.2}x)", ms, ratio);
    (ms, ratio)
}

fn main() {
    let traces = [
        ("sveltecomponent", "../../data/editing-traces/sequential_traces/ascii_only/sveltecomponent.json.gz"),
        ("rustcode", "../../data/editing-traces/sequential_traces/ascii_only/rustcode.json.gz"),
        ("seph-blog1", "../../data/editing-traces/sequential_traces/ascii_only/seph-blog1.json.gz"),
        ("automerge-paper", "../../data/editing-traces/sequential_traces/ascii_only/automerge-paper.json.gz"),
    ];
    
    let pair = KeyPair::generate();
    let key_pub = &pair.key_pub;
    
    let mut all_results: Vec<BenchResult> = vec![
        BenchResult { name: "diamond-types", times_ms: [0.0; 4], ratios: [1.0; 4] },
        BenchResult { name: "RgaBuf (original)", times_ms: [0.0; 4], ratios: [0.0; 4] },
        BenchResult { name: "YjsRga", times_ms: [0.0; 4], ratios: [0.0; 4] },
        BenchResult { name: "DiamondRga", times_ms: [0.0; 4], ratios: [0.0; 4] },
        BenchResult { name: "ColaRga", times_ms: [0.0; 4], ratios: [0.0; 4] },
        BenchResult { name: "JsonJoyRga", times_ms: [0.0; 4], ratios: [0.0; 4] },
        BenchResult { name: "LoroRga", times_ms: [0.0; 4], ratios: [0.0; 4] },
        BenchResult { name: "OptimizedRga", times_ms: [0.0; 4], ratios: [0.0; 4] },
    ];
    
    for (trace_idx, (trace_name, path)) in traces.iter().enumerate() {
        eprintln!("\n=== Loading {} ===", trace_name);
        let data = TestData::load(path);
        
        // diamond-types baseline
        eprint!("  diamond-types... ");
        let dt_time = bench_diamond_types(&data);
        let dt_ms = dt_time.as_secs_f64() * 1000.0;
        eprintln!("{:.2}ms (baseline)", dt_ms);
        all_results[0].times_ms[trace_idx] = dt_ms;
        
        // RgaBuf
        eprint!("  RgaBuf (original)... ");
        let t = bench_rgabuf(&data, key_pub);
        let ms = t.as_secs_f64() * 1000.0;
        let ratio = ms / dt_ms;
        eprintln!("{:.2}ms ({:.2}x)", ms, ratio);
        all_results[1].times_ms[trace_idx] = ms;
        all_results[1].ratios[trace_idx] = ratio;
        
        // All trait implementations
        let (ms, ratio) = bench_one::<YjsRga>("YjsRga", &data, key_pub, dt_ms);
        all_results[2].times_ms[trace_idx] = ms;
        all_results[2].ratios[trace_idx] = ratio;
        
        let (ms, ratio) = bench_one::<DiamondRga>("DiamondRga", &data, key_pub, dt_ms);
        all_results[3].times_ms[trace_idx] = ms;
        all_results[3].ratios[trace_idx] = ratio;
        
        let (ms, ratio) = bench_one::<ColaRga>("ColaRga", &data, key_pub, dt_ms);
        all_results[4].times_ms[trace_idx] = ms;
        all_results[4].ratios[trace_idx] = ratio;
        
        let (ms, ratio) = bench_one::<JsonJoyRga>("JsonJoyRga", &data, key_pub, dt_ms);
        all_results[5].times_ms[trace_idx] = ms;
        all_results[5].ratios[trace_idx] = ratio;
        
        let (ms, ratio) = bench_one::<LoroRga>("LoroRga", &data, key_pub, dt_ms);
        all_results[6].times_ms[trace_idx] = ms;
        all_results[6].ratios[trace_idx] = ratio;
        
        let (ms, ratio) = bench_one::<OptimizedRga>("OptimizedRga", &data, key_pub, dt_ms);
        all_results[7].times_ms[trace_idx] = ms;
        all_results[7].ratios[trace_idx] = ratio;
    }
    
    let results = all_results;
    
    // Print results
    println!("\n");
    println!("╔═══════════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                        RGA Implementation Comparison vs diamond-types                             ║");
    println!("╠═══════════════════════════════════════════════════════════════════════════════════════════════════╣");
    println!("║ Using real editing traces. All times in milliseconds.                                             ║");
    println!("║ Ratio = impl_time / diamond_types_time. Lower ratio = faster. <1.0 means faster than diamond.     ║");
    println!("╚═══════════════════════════════════════════════════════════════════════════════════════════════════╝");
    println!();
    
    // Time table
    println!("┌─────────────────────────────────────────────────────────────────────────────────────────────────┐");
    println!("│                                    ABSOLUTE TIMES (ms)                                          │");
    println!("├─────────────────────┬──────────────────┬──────────────────┬──────────────────┬──────────────────┤");
    println!("│ {:19} │ {:>16} │ {:>16} │ {:>16} │ {:>16} │", "Implementation", "sveltecomponent", "rustcode", "seph-blog1", "automerge-paper");
    println!("├─────────────────────┼──────────────────┼──────────────────┼──────────────────┼──────────────────┤");
    for r in &results {
        println!("│ {:19} │ {:>16.2} │ {:>16.2} │ {:>16.2} │ {:>16.2} │",
                 r.name, r.times_ms[0], r.times_ms[1], r.times_ms[2], r.times_ms[3]);
    }
    println!("└─────────────────────┴──────────────────┴──────────────────┴──────────────────┴──────────────────┘");
    println!();
    
    // Ratio table
    println!("┌─────────────────────────────────────────────────────────────────────────────────────────────────┐");
    println!("│                              RATIO vs diamond-types (lower = faster)                            │");
    println!("├─────────────────────┬──────────────────┬──────────────────┬──────────────────┬──────────────────┤");
    println!("│ {:19} │ {:>16} │ {:>16} │ {:>16} │ {:>16} │", "Implementation", "sveltecomponent", "rustcode", "seph-blog1", "automerge-paper");
    println!("├─────────────────────┼──────────────────┼──────────────────┼──────────────────┼──────────────────┤");
    for r in &results {
        let s0 = format_ratio(r.ratios[0]);
        let s1 = format_ratio(r.ratios[1]);
        let s2 = format_ratio(r.ratios[2]);
        let s3 = format_ratio(r.ratios[3]);
        println!("│ {:19} │ {:>16} │ {:>16} │ {:>16} │ {:>16} │",
                 r.name, s0, s1, s2, s3);
    }
    println!("└─────────────────────┴──────────────────┴──────────────────┴──────────────────┴──────────────────┘");
    println!();
    
    // Summary
    println!("Legend: ✅ faster than diamond-types, ⚠️  within 50%, ❌ slower");
}

fn format_ratio(ratio: f64) -> String {
    let icon = if ratio < 1.0 { "✅" } else if ratio < 1.5 { "⚠️ " } else { "❌" };
    format!("{} {:.2}x", icon, ratio)
}
