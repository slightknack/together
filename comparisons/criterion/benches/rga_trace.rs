// model = "claude-opus-4-5"
// created = "2026-01-30"
// modified = "2026-01-30"
// driver = "Isaac Clayton"

//! Benchmark comparing together's RGA against diamond-types using real editing traces.

use std::fs::File;
use std::io::BufReader;
use std::io::Read;

use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::black_box;
use criterion::criterion_group;
use criterion::criterion_main;
use flate2::bufread::GzDecoder;
use serde::Deserialize;

use together::crdt::rga::RgaBuf;
use together::key::KeyPair;

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

        return serde_json::from_slice(&raw_json).expect("failed to parse JSON");
    }

    fn patch_count(&self) -> usize {
        return self.txns.iter().map(|t| t.patches.len()).sum();
    }
}

/// Replay trace using together's RGA
fn replay_together(data: &TestData) -> String {
    let pair = KeyPair::generate();
    let mut rga = RgaBuf::new();

    for txn in &data.txns {
        for TestPatch(pos, del, ins) in &txn.patches {
            // Delete first (if any)
            if *del > 0 {
                rga.delete(*pos as u64, *del as u64);
            }
            // Then insert (if any)
            if !ins.is_empty() {
                rga.insert(&pair.key_pub, *pos as u64, ins.as_bytes());
            }
        }
    }

    return rga.to_string();
}

/// Replay trace using diamond-types
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

    return doc.branch.content().to_string();
}

/// Verify both implementations produce the same result
fn verify_consistency(name: &str, data: &TestData) {
    let together_result = replay_together(data);
    let diamond_result = replay_diamond(data);

    assert_eq!(
        together_result, data.end_content,
        "{}: together result doesn't match expected", name
    );
    assert_eq!(
        diamond_result, data.end_content,
        "{}: diamond result doesn't match expected", name
    );
    assert_eq!(
        together_result, diamond_result,
        "{}: together and diamond results differ", name
    );

    println!("{}: consistency verified ({} chars)", name, data.end_content.len());
}

fn bench_traces(c: &mut Criterion) {
    // Use ascii_only variants to avoid unicode position issues
    // Start with smaller traces; automerge-paper has 260k patches which is slow
    let traces = [
        ("sveltecomponent", "../../data/editing-traces/sequential_traces/ascii_only/sveltecomponent.json.gz"),
        ("rustcode", "../../data/editing-traces/sequential_traces/ascii_only/rustcode.json.gz"),
        ("seph-blog1", "../../data/editing-traces/sequential_traces/ascii_only/seph-blog1.json.gz"),
        ("automerge-paper", "../../data/editing-traces/sequential_traces/ascii_only/automerge-paper.json.gz"),
    ];

    // Load all traces
    let datasets: Vec<_> = traces
        .iter()
        .map(|(name, path)| {
            let data = TestData::load(path);
            println!("Loaded {}: {} patches", name, data.patch_count());
            (*name, data)
        })
        .collect();

    // Verify consistency for all traces
    println!("\n=== Verifying consistency ===");
    for (name, data) in &datasets {
        verify_consistency(name, data);
    }
    println!();

    // Benchmark together
    let mut group = c.benchmark_group("together");
    for (name, data) in &datasets {
        group.bench_with_input(
            BenchmarkId::new("replay", name),
            data,
            |b, data| b.iter(|| replay_together(black_box(data))),
        );
    }
    group.finish();

    // Benchmark diamond-types
    let mut group = c.benchmark_group("diamond-types");
    for (name, data) in &datasets {
        group.bench_with_input(
            BenchmarkId::new("replay", name),
            data,
            |b, data| b.iter(|| replay_diamond(black_box(data))),
        );
    }
    group.finish();
}

criterion_group!(benches, bench_traces);
criterion_main!(benches);
