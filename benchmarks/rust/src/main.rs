//! Comprehensive benchmark comparing CRDT libraries on editing traces.
//!
//! Libraries tested:
//! - Together
//! - diamond-types
//! - Automerge
//! - Yrs (Y.rs)
//! - Loro
//! - Cola

use std::fs::File;
use std::io::{BufReader, Read};
use std::time::{Duration, Instant};

use flate2::bufread::GzDecoder;
use serde::Deserialize;

/// A single patch: (position, delete_count, insert_content)
#[derive(Debug, Clone, Deserialize)]
struct TestPatch(usize, usize, String);

/// A transaction containing patches
#[derive(Debug, Clone, Deserialize)]
struct TestTxn {
    patches: Vec<TestPatch>,
}

/// The complete test data
#[derive(Debug, Clone, Deserialize)]
struct TestData {
    #[serde(rename = "startContent")]
    #[allow(dead_code)]
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

    fn patch_count(&self) -> usize {
        self.txns.iter().map(|t| t.patches.len()).sum()
    }
}

// =============================================================================
// Together benchmark
// =============================================================================

fn replay_together(data: &TestData) -> String {
    use together::crdt::rga::RgaBuf;
    use together::key::KeyPair;

    let pair = KeyPair::generate();
    let mut rga = RgaBuf::new();

    for txn in &data.txns {
        for TestPatch(pos, del, ins) in &txn.patches {
            if *del > 0 {
                rga.delete(*pos as u64, *del as u64);
            }
            if !ins.is_empty() {
                rga.insert(&pair.key_pub, *pos as u64, ins.as_bytes());
            }
        }
    }

    rga.to_string()
}

// =============================================================================
// diamond-types benchmark
// =============================================================================

fn replay_diamond(data: &TestData) -> String {
    use diamond_types::list::ListCRDT;

    let mut doc = ListCRDT::new();
    let agent = doc.get_or_create_agent_id("user");

    for txn in &data.txns {
        for TestPatch(pos, del, ins) in &txn.patches {
            if *del > 0 {
                doc.delete_without_content(agent, *pos..*pos + *del);
            }
            if !ins.is_empty() {
                doc.insert(agent, *pos, ins);
            }
        }
    }

    doc.branch.content().to_string()
}

// =============================================================================
// Automerge benchmark
// =============================================================================

fn replay_automerge(data: &TestData) -> String {
    use automerge::{AutoCommit, ObjType, ReadDoc, ROOT};
    use automerge::transaction::Transactable;

    let mut doc = AutoCommit::new();
    let text = doc.put_object(ROOT, "text", ObjType::Text).unwrap();

    for txn in &data.txns {
        for TestPatch(pos, del, ins) in &txn.patches {
            if *del > 0 {
                doc.splice_text(&text, *pos, *del as isize, "").unwrap();
            }
            if !ins.is_empty() {
                doc.splice_text(&text, *pos, 0, ins).unwrap();
            }
        }
    }

    doc.text(&text).unwrap()
}

// =============================================================================
// Yrs benchmark
// =============================================================================

fn replay_yrs(data: &TestData) -> String {
    use yrs::{Doc, GetString, Text, Transact};

    let doc = Doc::new();
    let text = doc.get_or_insert_text("text");

    for txn in &data.txns {
        let mut ytxn = doc.transact_mut();
        for TestPatch(pos, del, ins) in &txn.patches {
            if *del > 0 {
                text.remove_range(&mut ytxn, *pos as u32, *del as u32);
            }
            if !ins.is_empty() {
                text.insert(&mut ytxn, *pos as u32, ins);
            }
        }
    }

    let ytxn = doc.transact();
    text.get_string(&ytxn)
}

// =============================================================================
// Loro benchmark
// =============================================================================

fn replay_loro(data: &TestData) -> String {
    use loro::LoroDoc;

    let doc = LoroDoc::new();
    let text = doc.get_text("text");

    for txn in &data.txns {
        for TestPatch(pos, del, ins) in &txn.patches {
            if *del > 0 {
                text.delete(*pos, *del).unwrap();
            }
            if !ins.is_empty() {
                text.insert(*pos, ins).unwrap();
            }
        }
    }

    text.to_string()
}

// =============================================================================
// Cola benchmark
// =============================================================================

fn replay_cola(data: &TestData) -> String {
    use cola::Replica;

    let mut buffer = String::new();
    let mut crdt = Replica::new(1, 0);

    for txn in &data.txns {
        for TestPatch(pos, del, ins) in &txn.patches {
            if *del > 0 {
                buffer.replace_range(*pos..*pos + *del, "");
                let _ = crdt.deleted(*pos..*pos + *del);
            }
            if !ins.is_empty() {
                buffer.insert_str(*pos, ins);
                let _ = crdt.inserted(*pos, ins.len());
            }
        }
    }

    buffer
}

// =============================================================================
// Benchmarking infrastructure
// =============================================================================

fn benchmark<F>(_name: &str, iterations: usize, mut f: F) -> Duration
where
    F: FnMut(),
{
    // Warmup
    for _ in 0..2 {
        f();
    }

    // Collect timings
    let mut times: Vec<Duration> = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let start = Instant::now();
        f();
        times.push(start.elapsed());
    }

    // Return median
    times.sort();
    times[iterations / 2]
}

fn main() {
    let traces = [
        ("sveltecomponent", "../../data/editing-traces/sequential_traces/ascii_only/sveltecomponent.json.gz"),
        ("rustcode", "../../data/editing-traces/sequential_traces/ascii_only/rustcode.json.gz"),
        ("seph-blog1", "../../data/editing-traces/sequential_traces/ascii_only/seph-blog1.json.gz"),
        ("automerge-paper", "../../data/editing-traces/sequential_traces/ascii_only/automerge-paper.json.gz"),
    ];

    let iterations = 10;

    println!("Loading traces...");
    let datasets: Vec<_> = traces
        .iter()
        .map(|(name, path)| {
            let data = TestData::load(path);
            println!("  {}: {} patches", name, data.patch_count());
            (*name, data)
        })
        .collect();

    println!("\nVerifying correctness...");
    for (name, data) in &datasets {
        let together_result = replay_together(data);
        let diamond_result = replay_diamond(data);
        let automerge_result = replay_automerge(data);
        let yrs_result = replay_yrs(data);
        let loro_result = replay_loro(data);
        let cola_result = replay_cola(data);

        assert_eq!(together_result, data.end_content, "{}: together mismatch", name);
        assert_eq!(diamond_result, data.end_content, "{}: diamond-types mismatch", name);
        assert_eq!(automerge_result, data.end_content, "{}: automerge mismatch", name);
        assert_eq!(yrs_result, data.end_content, "{}: yrs mismatch", name);
        assert_eq!(loro_result, data.end_content, "{}: loro mismatch", name);
        assert_eq!(cola_result, data.end_content, "{}: cola mismatch", name);
        println!("  {}: all libraries produce correct output", name);
    }

    println!("\nRunning benchmarks ({} iterations, reporting median)...\n", iterations);

    // Results storage
    let mut results: Vec<(String, Vec<f64>)> = Vec::new();

    for (name, data) in &datasets {
        println!("Benchmarking {}...", name);
        let mut row: Vec<f64> = Vec::new();

        // Together
        let tg_time = benchmark("together", iterations, || {
            let _ = replay_together(data);
        });
        row.push(tg_time.as_secs_f64() * 1000.0);
        println!("  together: {:.2} ms", tg_time.as_secs_f64() * 1000.0);

        // diamond-types
        let dt_time = benchmark("diamond-types", iterations, || {
            let _ = replay_diamond(data);
        });
        row.push(dt_time.as_secs_f64() * 1000.0);
        println!("  diamond-types: {:.2} ms", dt_time.as_secs_f64() * 1000.0);

        // Automerge
        let am_time = benchmark("automerge", iterations, || {
            let _ = replay_automerge(data);
        });
        row.push(am_time.as_secs_f64() * 1000.0);
        println!("  automerge: {:.2} ms", am_time.as_secs_f64() * 1000.0);

        // Yrs
        let yrs_time = benchmark("yrs", iterations, || {
            let _ = replay_yrs(data);
        });
        row.push(yrs_time.as_secs_f64() * 1000.0);
        println!("  yrs: {:.2} ms", yrs_time.as_secs_f64() * 1000.0);

        // Loro
        let loro_time = benchmark("loro", iterations, || {
            let _ = replay_loro(data);
        });
        row.push(loro_time.as_secs_f64() * 1000.0);
        println!("  loro: {:.2} ms", loro_time.as_secs_f64() * 1000.0);

        // Cola
        let cola_time = benchmark("cola", iterations, || {
            let _ = replay_cola(data);
        });
        row.push(cola_time.as_secs_f64() * 1000.0);
        println!("  cola: {:.2} ms", cola_time.as_secs_f64() * 1000.0);

        results.push((name.to_string(), row));
        println!();
    }

    // Print table
    println!("=== Results Table ===\n");
    println!("| Trace | Together (ms) | diamond-types (ms) | Automerge (ms) | Yrs (ms) | Loro (ms) | Cola (ms) |");
    println!("|-------|---------------|-------------------|----------------|----------|-----------|-----------|");
    for (name, row) in &results {
        println!(
            "| `{}` | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} |",
            name, row[0], row[1], row[2], row[3], row[4], row[5]
        );
    }
}
