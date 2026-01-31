// model = "claude-opus-4-5"
// created = "2026-01-31"
// modified = "2026-01-31"
// driver = "Isaac Clayton"

//! Integration tests verifying Together produces identical output to diamond-types
//! on all standard editing traces from https://github.com/josephg/editing-traces

use std::fs::File;
use std::io::BufReader;
use std::io::Read;

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

        return serde_json::from_slice(&raw_json).expect("failed to parse JSON");
    }
}

fn replay_together(data: &TestData) -> String {
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

    return rga.to_string();
}

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

#[test]
fn sveltecomponent_matches_diamond_types() {
    let data = TestData::load("data/editing-traces/sequential_traces/ascii_only/sveltecomponent.json.gz");
    
    let together_result = replay_together(&data);
    let diamond_result = replay_diamond(&data);
    
    assert_eq!(together_result, data.end_content, "together output doesn't match expected");
    assert_eq!(diamond_result, data.end_content, "diamond output doesn't match expected");
    assert_eq!(together_result, diamond_result, "together and diamond outputs differ");
}

#[test]
fn rustcode_matches_diamond_types() {
    let data = TestData::load("data/editing-traces/sequential_traces/ascii_only/rustcode.json.gz");
    
    let together_result = replay_together(&data);
    let diamond_result = replay_diamond(&data);
    
    assert_eq!(together_result, data.end_content, "together output doesn't match expected");
    assert_eq!(diamond_result, data.end_content, "diamond output doesn't match expected");
    assert_eq!(together_result, diamond_result, "together and diamond outputs differ");
}

#[test]
fn seph_blog1_matches_diamond_types() {
    let data = TestData::load("data/editing-traces/sequential_traces/ascii_only/seph-blog1.json.gz");
    
    let together_result = replay_together(&data);
    let diamond_result = replay_diamond(&data);
    
    assert_eq!(together_result, data.end_content, "together output doesn't match expected");
    assert_eq!(diamond_result, data.end_content, "diamond output doesn't match expected");
    assert_eq!(together_result, diamond_result, "together and diamond outputs differ");
}

#[test]
fn automerge_paper_matches_diamond_types() {
    let data = TestData::load("data/editing-traces/sequential_traces/ascii_only/automerge-paper.json.gz");
    
    let together_result = replay_together(&data);
    let diamond_result = replay_diamond(&data);
    
    assert_eq!(together_result, data.end_content, "together output doesn't match expected");
    assert_eq!(diamond_result, data.end_content, "diamond output doesn't match expected");
    assert_eq!(together_result, diamond_result, "together and diamond outputs differ");
}
