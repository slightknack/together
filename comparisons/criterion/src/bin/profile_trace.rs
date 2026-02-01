use std::fs::File;
use std::io::BufReader;
use std::io::Read;
use std::time::Instant;

use flate2::bufread::GzDecoder;
use serde::Deserialize;

use together::crdt::rga::RgaBuf;
use together::key::KeyPair;

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

        return serde_json::from_slice(&raw_json).expect("failed to parse JSON");
    }
}

fn main() {
    // Try different traces to understand their characteristics
    for (name, path) in [
        ("sveltecomponent", "../../data/editing-traces/sequential_traces/ascii_only/sveltecomponent.json.gz"),
        ("rustcode", "../../data/editing-traces/sequential_traces/ascii_only/rustcode.json.gz"),
        ("seph-blog1", "../../data/editing-traces/sequential_traces/ascii_only/seph-blog1.json.gz"),
        ("automerge-paper", "../../data/editing-traces/sequential_traces/ascii_only/automerge-paper.json.gz"),
    ] {
        profile_trace(name, path);
        println!();
    }
}

fn profile_trace(name: &str, path: &str) {
    let data = TestData::load(path);
    
    let pair = KeyPair::generate();
    let mut rga = RgaBuf::new();
    
    let mut insert_count = 0;
    let mut delete_count = 0;
    let mut insert_time = std::time::Duration::ZERO;
    let mut delete_time = std::time::Duration::ZERO;
    
    for txn in &data.txns {
        for TestPatch(pos, del, ins) in &txn.patches {
            if *del > 0 {
                let t = Instant::now();
                rga.delete(*pos as u64, *del as u64);
                delete_time += t.elapsed();
                delete_count += 1;
            }
            if !ins.is_empty() {
                let t = Instant::now();
                rga.insert(&pair.key_pub, *pos as u64, ins.as_bytes());
                insert_time += t.elapsed();
                insert_count += 1;
            }
        }
    }
    
    println!("=== {} ===", name);
    println!("Inserts: {} ({:?}, {:?}/op)", insert_count, insert_time, insert_time / insert_count.max(1) as u32);
    println!("Deletes: {} ({:?}, {:?}/op)", delete_count, delete_time, delete_time / delete_count.max(1) as u32);
    println!("Total: {:?}, Doc len: {}", insert_time + delete_time, rga.len());
}
