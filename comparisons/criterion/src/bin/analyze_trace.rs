use std::fs::File;
use std::io::BufReader;
use std::io::Read;

use flate2::bufread::GzDecoder;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
struct TestPatch(usize, usize, String);

#[derive(Debug, Clone, Deserialize)]
struct TestTxn {
    patches: Vec<TestPatch>,
}

#[derive(Debug, Clone, Deserialize)]
struct TestData {
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

fn main() {
    for (name, path) in [
        ("sveltecomponent", "../../data/editing-traces/sequential_traces/ascii_only/sveltecomponent.json.gz"),
        ("rustcode", "../../data/editing-traces/sequential_traces/ascii_only/rustcode.json.gz"),
        ("seph-blog1", "../../data/editing-traces/sequential_traces/ascii_only/seph-blog1.json.gz"),
        ("automerge-paper", "../../data/editing-traces/sequential_traces/ascii_only/automerge-paper.json.gz"),
    ] {
        analyze_trace(name, path);
        println!();
    }
}

fn analyze_trace(name: &str, path: &str) {
    let data = TestData::load(path);
    
    let mut doc_len: i64 = 0;
    let mut insert_count = 0;
    let mut delete_count = 0;
    let mut sequential_insert = 0;
    let mut backward_insert = 0;
    let mut jump_insert = 0;
    let mut last_insert_end: Option<i64> = None;
    
    for txn in &data.txns {
        for TestPatch(pos, del, ins) in &txn.patches {
            if *del > 0 {
                doc_len -= *del as i64;
                delete_count += 1;
                last_insert_end = None; // Delete breaks sequence
            }
            if !ins.is_empty() {
                insert_count += 1;
                let pos_i64 = *pos as i64;
                
                if let Some(last_end) = last_insert_end {
                    if pos_i64 == last_end {
                        sequential_insert += 1;
                    } else if pos_i64 == last_end - 1 {
                        backward_insert += 1;
                    } else {
                        jump_insert += 1;
                    }
                } else {
                    jump_insert += 1;
                }
                
                last_insert_end = Some(pos_i64 + ins.len() as i64);
                doc_len += ins.len() as i64;
            }
        }
    }
    
    println!("=== {} ===", name);
    println!("Inserts: {} (seq: {}, back: {}, jump: {})", 
             insert_count, sequential_insert, backward_insert, jump_insert);
    println!("Deletes: {}", delete_count);
    println!("Jump ratio: {:.1}%", 100.0 * jump_insert as f64 / insert_count as f64);
}
