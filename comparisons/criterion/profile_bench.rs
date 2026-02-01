use std::fs::File;
use std::io::BufReader;
use std::io::Read;
use std::time::Instant;

use diamond_types::list::ListCRDT;
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
    #[allow(dead_code)]
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
    let traces = [
        ("sveltecomponent", "../../data/editing-traces/sequential_traces/ascii_only/sveltecomponent.json.gz"),
        ("rustcode", "../../data/editing-traces/sequential_traces/ascii_only/rustcode.json.gz"),
        ("seph-blog1", "../../data/editing-traces/sequential_traces/ascii_only/seph-blog1.json.gz"),
        ("automerge-paper", "../../data/editing-traces/sequential_traces/ascii_only/automerge-paper.json.gz"),
    ];
    
    println!("Running 100 iterations each for stable timing...\n");
    
    for (name, path) in &traces {
        let data = TestData::load(path);
        let pair = KeyPair::generate();
        let iterations = 100;
        
        // Together
        let mut together_times = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let mut rga = RgaBuf::new();
            let start = Instant::now();
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
            rga.flush();
            together_times.push(start.elapsed());
        }
        
        // Diamond
        let mut diamond_times = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let mut doc = ListCRDT::new();
            let agent = doc.get_or_create_agent_id("user");
            let start = Instant::now();
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
            diamond_times.push(start.elapsed());
        }
        
        together_times.sort();
        diamond_times.sort();
        
        let t_median = together_times[iterations / 2];
        let d_median = diamond_times[iterations / 2];
        let ratio = t_median.as_secs_f64() / d_median.as_secs_f64();
        
        println!("{}: together={:?}, diamond={:?}, ratio={:.2}x", name, t_median, d_median, ratio);
    }
}
