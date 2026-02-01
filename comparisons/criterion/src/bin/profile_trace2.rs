use std::fs::File;
use std::io::BufReader;
use std::io::Read;
use std::time::Instant;

use flate2::bufread::GzDecoder;
use serde::Deserialize;

use together::crdt::rga::RgaBuf;
use together::key::KeyPair;
use diamond_types::list::ListCRDT;

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

        return serde_json::from_slice(&raw_json).expect("failed to parse JSON");
    }
}

fn main() {
    let path = "../../data/editing-traces/sequential_traces/ascii_only/seph-blog1.json.gz";
    let data = TestData::load(path);
    
    // Together - inserts only
    let pair = KeyPair::generate();
    let mut rga = RgaBuf::new();
    let start = Instant::now();
    for txn in &data.txns {
        for TestPatch(pos, _del, ins) in &txn.patches {
            if !ins.is_empty() {
                let actual_pos = (*pos as u64).min(rga.len());
                rga.insert(&pair.key_pub, actual_pos, ins.as_bytes());
            }
        }
    }
    let together_time = start.elapsed();
    
    // Diamond - inserts only
    let mut doc = ListCRDT::new();
    let agent = doc.get_or_create_agent_id("user");
    let start = Instant::now();
    for txn in &data.txns {
        for TestPatch(pos, _del, ins) in &txn.patches {
            if !ins.is_empty() {
                let actual_pos = (*pos).min(doc.len());
                doc.insert(agent, actual_pos, ins);
            }
        }
    }
    let diamond_time = start.elapsed();
    
    println!("Inserts only:");
    println!("  Together: {:?}", together_time);
    println!("  Diamond:  {:?}", diamond_time);
    println!("  Ratio:    {:.1}x", together_time.as_secs_f64() / diamond_time.as_secs_f64());
}
