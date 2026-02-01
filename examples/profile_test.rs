use together::crdt::rga::RgaBuf;
use together::key::KeyPair;
use std::time::Instant;

fn main() {
    let pair = KeyPair::generate();
    let mut rga = RgaBuf::new();
    
    // Simulate typing 100k characters one at a time at end
    let start = Instant::now();
    for i in 0..100_000 {
        rga.insert(&pair.key_pub, i as u64, b"x");
    }
    let elapsed = start.elapsed();
    
    println!("100k sequential inserts at end: {:?}", elapsed);
    println!("Per insert: {:?}", elapsed / 100_000);
    println!("Doc length: {}", rga.len());
    
    // Now test random position inserts
    let mut rga2 = RgaBuf::new();
    let start2 = Instant::now();
    for i in 0..10_000 {
        let pos = if rga2.len() == 0 { 0 } else { (i * 7) % (rga2.len() as usize) };
        rga2.insert(&pair.key_pub, pos as u64, b"x");
    }
    let elapsed2 = start2.elapsed();
    
    println!("\n10k random position inserts: {:?}", elapsed2);
    println!("Per insert: {:?}", elapsed2 / 10_000);
}
