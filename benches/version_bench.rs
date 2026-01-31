// Version API benchmark - measures version() and to_string_at() performance

use std::time::Instant;

use together::crdt::rga::Rga;
use together::key::KeyPair;

fn main() {
    let pair = KeyPair::generate();
    
    // Build a document with many edits
    let mut rga = Rga::new();
    let num_edits = 10000;
    
    println!("Building document with {} edits...", num_edits);
    for i in 0..num_edits {
        let content = format!("edit{} ", i);
        rga.insert(&pair.key_pub, rga.len(), content.as_bytes());
    }
    println!("Document length: {} chars, {} spans", rga.len(), rga.span_count());
    
    // Benchmark version() - taking snapshots
    println!("\n=== version() benchmark ===");
    let iterations = 1000;
    
    let start = Instant::now();
    let mut versions = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        versions.push(rga.version());
    }
    let version_time = start.elapsed();
    println!("  {} iterations: {:?}", iterations, version_time);
    println!("  per call: {:?}", version_time / iterations as u32);
    
    // Benchmark to_string_at() - reading historical versions
    println!("\n=== to_string_at() benchmark ===");
    let version = &versions[0];
    
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = rga.to_string_at(version);
    }
    let read_time = start.elapsed();
    println!("  {} iterations: {:?}", iterations, read_time);
    println!("  per call: {:?}", read_time / iterations as u32);
    
    // Benchmark slice_at() - reading slices from historical versions
    println!("\n=== slice_at() benchmark ===");
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = rga.slice_at(0, 1000, version);
    }
    let slice_time = start.elapsed();
    println!("  {} iterations: {:?}", iterations, slice_time);
    println!("  per call: {:?}", slice_time / iterations as u32);
    
    // Benchmark len_at() - should be O(1)
    println!("\n=== len_at() benchmark ===");
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = rga.len_at(version);
    }
    let len_time = start.elapsed();
    println!("  {} iterations: {:?}", iterations, len_time);
    println!("  per call: {:?}", len_time / iterations as u32);
    
    // Memory usage estimate
    println!("\n=== Memory estimate ===");
    println!("  versions stored: {}", versions.len());
    println!("  spans per version: {}", rga.span_count());
}
