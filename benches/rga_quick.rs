// Quick benchmark for getting summary results across all implementations

use std::time::Instant;

use together::crdt::rga_trait::Rga;
use together::crdt::yjs::YjsRga;
use together::crdt::diamond::DiamondRga;
use together::crdt::cola::ColaRga;
use together::crdt::json_joy::JsonJoyRga;
use together::crdt::loro::LoroRga;
use together::crdt::rga_optimized::OptimizedRga;
use together::key::KeyPair;

use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;

fn time_ops<F: Fn() -> u64>(name: &str, f: F, iterations: usize) -> f64 {
    // Warmup
    for _ in 0..3 {
        let _ = f();
    }
    
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = f();
    }
    let elapsed = start.elapsed();
    let per_op = elapsed.as_nanos() as f64 / iterations as f64;
    per_op
}

macro_rules! bench_impl {
    ($name:expr, $rga:ty, $user:expr, $content:expr, $rng_seed:expr) => {{
        let user = $user;
        let content = $content;
        
        // Sequential forward (100 chars)
        let seq_fwd = time_ops("seq_fwd", || {
            let mut rga = <$rga>::new();
            for (i, byte) in content.iter().take(100).enumerate() {
                rga.insert(&user.key_pub, i as u64, &[*byte]);
            }
            rga.len()
        }, 100);
        
        // Random inserts (100 chars)
        let random_ins = time_ops("random_ins", || {
            let mut rga = <$rga>::new();
            let mut rng = StdRng::seed_from_u64($rng_seed);
            for byte in content.iter().take(100) {
                let len = rga.len();
                let pos = if len == 0 { 0 } else { rng.gen_range(0..=len) };
                rga.insert(&user.key_pub, pos, &[*byte]);
            }
            rga.len()
        }, 100);
        
        // Random deletes (100 chars)
        let random_del = time_ops("random_del", || {
            let mut rga = <$rga>::new();
            rga.insert(&user.key_pub, 0, content);
            let mut rng = StdRng::seed_from_u64($rng_seed);
            for _ in 0..100 {
                let len = rga.len();
                if len == 0 { break; }
                let pos = rng.gen_range(0..len);
                rga.delete(pos, 1);
            }
            rga.len()
        }, 100);
        
        // Merge (two 100-char docs)
        let content_a: Vec<u8> = (0..100).map(|i| b'A' + (i % 26) as u8).collect();
        let content_b: Vec<u8> = (0..100).map(|i| b'a' + (i % 26) as u8).collect();
        let user2 = KeyPair::generate();
        let merge_time = time_ops("merge", || {
            let mut rga_a = <$rga>::new();
            let mut rga_b = <$rga>::new();
            rga_a.insert(&user.key_pub, 0, &content_a);
            rga_b.insert(&user2.key_pub, 0, &content_b);
            rga_a.merge(&rga_b);
            rga_a.len()
        }, 100);
        
        println!(
            "| {:12} | {:>10.0} | {:>10.0} | {:>10.0} | {:>10.0} |",
            $name,
            seq_fwd / 1000.0,
            random_ins / 1000.0,
            random_del / 1000.0,
            merge_time / 1000.0
        );
    }};
}

fn main() {
    println!("\n=== RGA Implementation Comparison (100 char operations) ===\n");
    println!("All times in microseconds (us)\n");
    println!("| {:12} | {:>10} | {:>10} | {:>10} | {:>10} |", "Impl", "Seq Fwd", "Rand Ins", "Rand Del", "Merge");
    println!("|--------------|------------|------------|------------|------------|");
    
    let user = KeyPair::generate();
    let content: Vec<u8> = (0..1000).map(|i| b'a' + (i % 26) as u8).collect();
    
    bench_impl!("YjsRga", YjsRga, &user, &content, 42);
    bench_impl!("DiamondRga", DiamondRga, &user, &content, 42);
    bench_impl!("ColaRga", ColaRga, &user, &content, 42);
    bench_impl!("JsonJoyRga", JsonJoyRga, &user, &content, 42);
    bench_impl!("LoroRga", LoroRga, &user, &content, 42);
    bench_impl!("OptimizedRga", OptimizedRga, &user, &content, 42);
    
    println!();
    
    // Larger scale test (1000 chars)
    println!("\n=== RGA Implementation Comparison (1000 char operations) ===\n");
    println!("All times in microseconds (us)\n");
    println!("| {:12} | {:>10} | {:>10} | {:>10} | {:>10} |", "Impl", "Seq Fwd", "Rand Ins", "Rand Del", "Merge");
    println!("|--------------|------------|------------|------------|------------|");
    
    let content_1k: Vec<u8> = (0..1000).map(|i| b'a' + (i % 26) as u8).collect();
    
    // Sequential forward 1000
    let seq_fwd_1k = |name: &str, f: Box<dyn Fn() -> u64>| -> f64 {
        time_ops(name, f, 20)
    };
    
    {
        let user = KeyPair::generate();
        let content = content_1k.clone();
        let ns = seq_fwd_1k("YjsRga", Box::new(move || {
            let mut rga = YjsRga::new();
            for (i, byte) in content.iter().enumerate() {
                rga.insert(&user.key_pub, i as u64, &[*byte]);
            }
            rga.len()
        }));
        print!("| {:12} | {:>10.0} |", "YjsRga", ns / 1000.0);
    }
    
    {
        let user = KeyPair::generate();
        let content = content_1k.clone();
        let mut rng = StdRng::seed_from_u64(42);
        let ns = time_ops("rand", || {
            let mut rga = YjsRga::new();
            let mut rng2 = StdRng::seed_from_u64(42);
            for byte in content.iter() {
                let len = rga.len();
                let pos = if len == 0 { 0 } else { rng2.gen_range(0..=len) };
                rga.insert(&user.key_pub, pos, &[*byte]);
            }
            rga.len()
        }, 20);
        print!(" {:>10.0} |", ns / 1000.0);
    }
    
    {
        let user = KeyPair::generate();
        let content = content_1k.clone();
        let ns = time_ops("del", || {
            let mut rga = YjsRga::new();
            rga.insert(&user.key_pub, 0, &content);
            let mut rng = StdRng::seed_from_u64(42);
            for _ in 0..1000 {
                let len = rga.len();
                if len == 0 { break; }
                let pos = rng.gen_range(0..len);
                rga.delete(pos, 1);
            }
            rga.len()
        }, 20);
        print!(" {:>10.0} |", ns / 1000.0);
    }
    
    {
        let user1 = KeyPair::generate();
        let user2 = KeyPair::generate();
        let content_a: Vec<u8> = (0..1000).map(|i| b'A' + (i % 26) as u8).collect();
        let content_b: Vec<u8> = (0..1000).map(|i| b'a' + (i % 26) as u8).collect();
        let ns = time_ops("merge", || {
            let mut rga_a = YjsRga::new();
            let mut rga_b = YjsRga::new();
            rga_a.insert(&user1.key_pub, 0, &content_a);
            rga_b.insert(&user2.key_pub, 0, &content_b);
            rga_a.merge(&rga_b);
            rga_a.len()
        }, 20);
        println!(" {:>10.0} |", ns / 1000.0);
    }
    
    // DiamondRga 1000
    {
        let user = KeyPair::generate();
        let content = content_1k.clone();
        let ns = time_ops("seq", || {
            let mut rga = DiamondRga::new();
            for (i, byte) in content.iter().enumerate() {
                rga.insert(&user.key_pub, i as u64, &[*byte]);
            }
            rga.len()
        }, 20);
        print!("| {:12} | {:>10.0} |", "DiamondRga", ns / 1000.0);
    }
    
    {
        let user = KeyPair::generate();
        let content = content_1k.clone();
        let ns = time_ops("rand", || {
            let mut rga = DiamondRga::new();
            let mut rng = StdRng::seed_from_u64(42);
            for byte in content.iter() {
                let len = rga.len();
                let pos = if len == 0 { 0 } else { rng.gen_range(0..=len) };
                rga.insert(&user.key_pub, pos, &[*byte]);
            }
            rga.len()
        }, 20);
        print!(" {:>10.0} |", ns / 1000.0);
    }
    
    {
        let user = KeyPair::generate();
        let content = content_1k.clone();
        let ns = time_ops("del", || {
            let mut rga = DiamondRga::new();
            rga.insert(&user.key_pub, 0, &content);
            let mut rng = StdRng::seed_from_u64(42);
            for _ in 0..1000 {
                let len = rga.len();
                if len == 0 { break; }
                let pos = rng.gen_range(0..len);
                rga.delete(pos, 1);
            }
            rga.len()
        }, 20);
        print!(" {:>10.0} |", ns / 1000.0);
    }
    
    {
        let user1 = KeyPair::generate();
        let user2 = KeyPair::generate();
        let content_a: Vec<u8> = (0..1000).map(|i| b'A' + (i % 26) as u8).collect();
        let content_b: Vec<u8> = (0..1000).map(|i| b'a' + (i % 26) as u8).collect();
        let ns = time_ops("merge", || {
            let mut rga_a = DiamondRga::new();
            let mut rga_b = DiamondRga::new();
            rga_a.insert(&user1.key_pub, 0, &content_a);
            rga_b.insert(&user2.key_pub, 0, &content_b);
            rga_a.merge(&rga_b);
            rga_a.len()
        }, 20);
        println!(" {:>10.0} |", ns / 1000.0);
    }
    
    // ColaRga 1000 - skip random insert due to O(n^2)
    {
        let user = KeyPair::generate();
        let content = content_1k.clone();
        let ns = time_ops("seq", || {
            let mut rga = ColaRga::new();
            for (i, byte) in content.iter().enumerate() {
                rga.insert(&user.key_pub, i as u64, &[*byte]);
            }
            rga.len()
        }, 20);
        print!("| {:12} | {:>10.0} |", "ColaRga", ns / 1000.0);
    }
    
    print!(" {:>10} |", "(slow)");
    
    {
        let user = KeyPair::generate();
        let content = content_1k.clone();
        let ns = time_ops("del", || {
            let mut rga = ColaRga::new();
            rga.insert(&user.key_pub, 0, &content);
            let mut rng = StdRng::seed_from_u64(42);
            for _ in 0..1000 {
                let len = rga.len();
                if len == 0 { break; }
                let pos = rng.gen_range(0..len);
                rga.delete(pos, 1);
            }
            rga.len()
        }, 20);
        print!(" {:>10.0} |", ns / 1000.0);
    }
    
    {
        let user1 = KeyPair::generate();
        let user2 = KeyPair::generate();
        let content_a: Vec<u8> = (0..1000).map(|i| b'A' + (i % 26) as u8).collect();
        let content_b: Vec<u8> = (0..1000).map(|i| b'a' + (i % 26) as u8).collect();
        let ns = time_ops("merge", || {
            let mut rga_a = ColaRga::new();
            let mut rga_b = ColaRga::new();
            rga_a.insert(&user1.key_pub, 0, &content_a);
            rga_b.insert(&user2.key_pub, 0, &content_b);
            rga_a.merge(&rga_b);
            rga_a.len()
        }, 20);
        println!(" {:>10.0} |", ns / 1000.0);
    }
    
    // JsonJoyRga 1000
    {
        let user = KeyPair::generate();
        let content = content_1k.clone();
        let ns = time_ops("seq", || {
            let mut rga = JsonJoyRga::new();
            for (i, byte) in content.iter().enumerate() {
                rga.insert(&user.key_pub, i as u64, &[*byte]);
            }
            rga.len()
        }, 20);
        print!("| {:12} | {:>10.0} |", "JsonJoyRga", ns / 1000.0);
    }
    
    {
        let user = KeyPair::generate();
        let content = content_1k.clone();
        let ns = time_ops("rand", || {
            let mut rga = JsonJoyRga::new();
            let mut rng = StdRng::seed_from_u64(42);
            for byte in content.iter() {
                let len = rga.len();
                let pos = if len == 0 { 0 } else { rng.gen_range(0..=len) };
                rga.insert(&user.key_pub, pos, &[*byte]);
            }
            rga.len()
        }, 20);
        print!(" {:>10.0} |", ns / 1000.0);
    }
    
    {
        let user = KeyPair::generate();
        let content = content_1k.clone();
        let ns = time_ops("del", || {
            let mut rga = JsonJoyRga::new();
            rga.insert(&user.key_pub, 0, &content);
            let mut rng = StdRng::seed_from_u64(42);
            for _ in 0..1000 {
                let len = rga.len();
                if len == 0 { break; }
                let pos = rng.gen_range(0..len);
                rga.delete(pos, 1);
            }
            rga.len()
        }, 20);
        print!(" {:>10.0} |", ns / 1000.0);
    }
    
    {
        let user1 = KeyPair::generate();
        let user2 = KeyPair::generate();
        let content_a: Vec<u8> = (0..1000).map(|i| b'A' + (i % 26) as u8).collect();
        let content_b: Vec<u8> = (0..1000).map(|i| b'a' + (i % 26) as u8).collect();
        let ns = time_ops("merge", || {
            let mut rga_a = JsonJoyRga::new();
            let mut rga_b = JsonJoyRga::new();
            rga_a.insert(&user1.key_pub, 0, &content_a);
            rga_b.insert(&user2.key_pub, 0, &content_b);
            rga_a.merge(&rga_b);
            rga_a.len()
        }, 20);
        println!(" {:>10.0} |", ns / 1000.0);
    }
    
    // LoroRga 1000
    {
        let user = KeyPair::generate();
        let content = content_1k.clone();
        let ns = time_ops("seq", || {
            let mut rga = LoroRga::new();
            for (i, byte) in content.iter().enumerate() {
                rga.insert(&user.key_pub, i as u64, &[*byte]);
            }
            rga.len()
        }, 20);
        print!("| {:12} | {:>10.0} |", "LoroRga", ns / 1000.0);
    }
    
    {
        let user = KeyPair::generate();
        let content = content_1k.clone();
        let ns = time_ops("rand", || {
            let mut rga = LoroRga::new();
            let mut rng = StdRng::seed_from_u64(42);
            for byte in content.iter() {
                let len = rga.len();
                let pos = if len == 0 { 0 } else { rng.gen_range(0..=len) };
                rga.insert(&user.key_pub, pos, &[*byte]);
            }
            rga.len()
        }, 20);
        print!(" {:>10.0} |", ns / 1000.0);
    }
    
    {
        let user = KeyPair::generate();
        let content = content_1k.clone();
        let ns = time_ops("del", || {
            let mut rga = LoroRga::new();
            rga.insert(&user.key_pub, 0, &content);
            let mut rng = StdRng::seed_from_u64(42);
            for _ in 0..1000 {
                let len = rga.len();
                if len == 0 { break; }
                let pos = rng.gen_range(0..len);
                rga.delete(pos, 1);
            }
            rga.len()
        }, 20);
        print!(" {:>10.0} |", ns / 1000.0);
    }
    
    {
        let user1 = KeyPair::generate();
        let user2 = KeyPair::generate();
        let content_a: Vec<u8> = (0..1000).map(|i| b'A' + (i % 26) as u8).collect();
        let content_b: Vec<u8> = (0..1000).map(|i| b'a' + (i % 26) as u8).collect();
        let ns = time_ops("merge", || {
            let mut rga_a = LoroRga::new();
            let mut rga_b = LoroRga::new();
            rga_a.insert(&user1.key_pub, 0, &content_a);
            rga_b.insert(&user2.key_pub, 0, &content_b);
            rga_a.merge(&rga_b);
            rga_a.len()
        }, 20);
        println!(" {:>10.0} |", ns / 1000.0);
    }
    
    // OptimizedRga 1000
    {
        let user = KeyPair::generate();
        let content = content_1k.clone();
        let ns = time_ops("seq", || {
            let mut rga = OptimizedRga::new();
            for (i, byte) in content.iter().enumerate() {
                rga.insert(&user.key_pub, i as u64, &[*byte]);
            }
            rga.len()
        }, 20);
        print!("| {:12} | {:>10.0} |", "OptimizedRga", ns / 1000.0);
    }
    
    {
        let user = KeyPair::generate();
        let content = content_1k.clone();
        let ns = time_ops("rand", || {
            let mut rga = OptimizedRga::new();
            let mut rng = StdRng::seed_from_u64(42);
            for byte in content.iter() {
                let len = rga.len();
                let pos = if len == 0 { 0 } else { rng.gen_range(0..=len) };
                rga.insert(&user.key_pub, pos, &[*byte]);
            }
            rga.len()
        }, 20);
        print!(" {:>10.0} |", ns / 1000.0);
    }
    
    {
        let user = KeyPair::generate();
        let content = content_1k.clone();
        let ns = time_ops("del", || {
            let mut rga = OptimizedRga::new();
            rga.insert(&user.key_pub, 0, &content);
            let mut rng = StdRng::seed_from_u64(42);
            for _ in 0..1000 {
                let len = rga.len();
                if len == 0 { break; }
                let pos = rng.gen_range(0..len);
                rga.delete(pos, 1);
            }
            rga.len()
        }, 20);
        print!(" {:>10.0} |", ns / 1000.0);
    }
    
    {
        let user1 = KeyPair::generate();
        let user2 = KeyPair::generate();
        let content_a: Vec<u8> = (0..1000).map(|i| b'A' + (i % 26) as u8).collect();
        let content_b: Vec<u8> = (0..1000).map(|i| b'a' + (i % 26) as u8).collect();
        let ns = time_ops("merge", || {
            let mut rga_a = OptimizedRga::new();
            let mut rga_b = OptimizedRga::new();
            rga_a.insert(&user1.key_pub, 0, &content_a);
            rga_b.insert(&user2.key_pub, 0, &content_b);
            rga_a.merge(&rga_b);
            rga_a.len()
        }, 20);
        println!(" {:>10.0} |", ns / 1000.0);
    }
    
    println!();
    
    // Memory/fragmentation test
    println!("\n=== Span Count After Operations (fragmentation measure) ===\n");
    println!("| {:12} | {:>10} | {:>10} | {:>10} |", "Impl", "1K Seq Ins", "1K Rand Ins", "1K Merged");
    println!("|--------------|------------|------------|------------|");
    
    // YjsRga
    {
        let user = KeyPair::generate();
        let mut rga = YjsRga::new();
        for (i, byte) in content_1k.iter().enumerate() {
            rga.insert(&user.key_pub, i as u64, &[*byte]);
        }
        let seq_spans = rga.span_count();
        
        let mut rga = YjsRga::new();
        let mut rng = StdRng::seed_from_u64(42);
        for byte in content_1k.iter() {
            let len = rga.len();
            let pos = if len == 0 { 0 } else { rng.gen_range(0..=len) };
            rga.insert(&user.key_pub, pos, &[*byte]);
        }
        let rand_spans = rga.span_count();
        
        let user2 = KeyPair::generate();
        let mut rga_a = YjsRga::new();
        let mut rga_b = YjsRga::new();
        let content_a: Vec<u8> = (0..1000).map(|i| b'A' + (i % 26) as u8).collect();
        let content_b: Vec<u8> = (0..1000).map(|i| b'a' + (i % 26) as u8).collect();
        rga_a.insert(&user.key_pub, 0, &content_a);
        rga_b.insert(&user2.key_pub, 0, &content_b);
        rga_a.merge(&rga_b);
        let merged_spans = rga_a.span_count();
        
        println!("| {:12} | {:>10} | {:>10} | {:>10} |", "YjsRga", seq_spans, rand_spans, merged_spans);
    }
    
    // DiamondRga
    {
        let user = KeyPair::generate();
        let mut rga = DiamondRga::new();
        for (i, byte) in content_1k.iter().enumerate() {
            rga.insert(&user.key_pub, i as u64, &[*byte]);
        }
        let seq_spans = rga.span_count();
        
        let mut rga = DiamondRga::new();
        let mut rng = StdRng::seed_from_u64(42);
        for byte in content_1k.iter() {
            let len = rga.len();
            let pos = if len == 0 { 0 } else { rng.gen_range(0..=len) };
            rga.insert(&user.key_pub, pos, &[*byte]);
        }
        let rand_spans = rga.span_count();
        
        let user2 = KeyPair::generate();
        let mut rga_a = DiamondRga::new();
        let mut rga_b = DiamondRga::new();
        let content_a: Vec<u8> = (0..1000).map(|i| b'A' + (i % 26) as u8).collect();
        let content_b: Vec<u8> = (0..1000).map(|i| b'a' + (i % 26) as u8).collect();
        rga_a.insert(&user.key_pub, 0, &content_a);
        rga_b.insert(&user2.key_pub, 0, &content_b);
        rga_a.merge(&rga_b);
        let merged_spans = rga_a.span_count();
        
        println!("| {:12} | {:>10} | {:>10} | {:>10} |", "DiamondRga", seq_spans, rand_spans, merged_spans);
    }
    
    // ColaRga
    {
        let user = KeyPair::generate();
        let mut rga = ColaRga::new();
        for (i, byte) in content_1k.iter().enumerate() {
            rga.insert(&user.key_pub, i as u64, &[*byte]);
        }
        let seq_spans = rga.span_count();
        
        // Skip random for ColaRga (too slow)
        let rand_spans = "(slow)";
        
        let user2 = KeyPair::generate();
        let mut rga_a = ColaRga::new();
        let mut rga_b = ColaRga::new();
        let content_a: Vec<u8> = (0..1000).map(|i| b'A' + (i % 26) as u8).collect();
        let content_b: Vec<u8> = (0..1000).map(|i| b'a' + (i % 26) as u8).collect();
        rga_a.insert(&user.key_pub, 0, &content_a);
        rga_b.insert(&user2.key_pub, 0, &content_b);
        rga_a.merge(&rga_b);
        let merged_spans = rga_a.span_count();
        
        println!("| {:12} | {:>10} | {:>10} | {:>10} |", "ColaRga", seq_spans, rand_spans, merged_spans);
    }
    
    // JsonJoyRga
    {
        let user = KeyPair::generate();
        let mut rga = JsonJoyRga::new();
        for (i, byte) in content_1k.iter().enumerate() {
            rga.insert(&user.key_pub, i as u64, &[*byte]);
        }
        let seq_spans = rga.span_count();
        
        let mut rga = JsonJoyRga::new();
        let mut rng = StdRng::seed_from_u64(42);
        for byte in content_1k.iter() {
            let len = rga.len();
            let pos = if len == 0 { 0 } else { rng.gen_range(0..=len) };
            rga.insert(&user.key_pub, pos, &[*byte]);
        }
        let rand_spans = rga.span_count();
        
        let user2 = KeyPair::generate();
        let mut rga_a = JsonJoyRga::new();
        let mut rga_b = JsonJoyRga::new();
        let content_a: Vec<u8> = (0..1000).map(|i| b'A' + (i % 26) as u8).collect();
        let content_b: Vec<u8> = (0..1000).map(|i| b'a' + (i % 26) as u8).collect();
        rga_a.insert(&user.key_pub, 0, &content_a);
        rga_b.insert(&user2.key_pub, 0, &content_b);
        rga_a.merge(&rga_b);
        let merged_spans = rga_a.span_count();
        
        println!("| {:12} | {:>10} | {:>10} | {:>10} |", "JsonJoyRga", seq_spans, rand_spans, merged_spans);
    }
    
    // LoroRga
    {
        let user = KeyPair::generate();
        let mut rga = LoroRga::new();
        for (i, byte) in content_1k.iter().enumerate() {
            rga.insert(&user.key_pub, i as u64, &[*byte]);
        }
        let seq_spans = rga.span_count();
        
        let mut rga = LoroRga::new();
        let mut rng = StdRng::seed_from_u64(42);
        for byte in content_1k.iter() {
            let len = rga.len();
            let pos = if len == 0 { 0 } else { rng.gen_range(0..=len) };
            rga.insert(&user.key_pub, pos, &[*byte]);
        }
        let rand_spans = rga.span_count();
        
        let user2 = KeyPair::generate();
        let mut rga_a = LoroRga::new();
        let mut rga_b = LoroRga::new();
        let content_a: Vec<u8> = (0..1000).map(|i| b'A' + (i % 26) as u8).collect();
        let content_b: Vec<u8> = (0..1000).map(|i| b'a' + (i % 26) as u8).collect();
        rga_a.insert(&user.key_pub, 0, &content_a);
        rga_b.insert(&user2.key_pub, 0, &content_b);
        rga_a.merge(&rga_b);
        let merged_spans = rga_a.span_count();
        
        println!("| {:12} | {:>10} | {:>10} | {:>10} |", "LoroRga", seq_spans, rand_spans, merged_spans);
    }
    
    // OptimizedRga
    {
        let user = KeyPair::generate();
        let mut rga = OptimizedRga::new();
        for (i, byte) in content_1k.iter().enumerate() {
            rga.insert(&user.key_pub, i as u64, &[*byte]);
        }
        let seq_spans = rga.span_count();
        
        let mut rga = OptimizedRga::new();
        let mut rng = StdRng::seed_from_u64(42);
        for byte in content_1k.iter() {
            let len = rga.len();
            let pos = if len == 0 { 0 } else { rng.gen_range(0..=len) };
            rga.insert(&user.key_pub, pos, &[*byte]);
        }
        let rand_spans = rga.span_count();
        
        let user2 = KeyPair::generate();
        let mut rga_a = OptimizedRga::new();
        let mut rga_b = OptimizedRga::new();
        let content_a: Vec<u8> = (0..1000).map(|i| b'A' + (i % 26) as u8).collect();
        let content_b: Vec<u8> = (0..1000).map(|i| b'a' + (i % 26) as u8).collect();
        rga_a.insert(&user.key_pub, 0, &content_a);
        rga_b.insert(&user2.key_pub, 0, &content_b);
        rga_a.merge(&rga_b);
        let merged_spans = rga_a.span_count();
        
        println!("| {:12} | {:>10} | {:>10} | {:>10} |", "OptimizedRga", seq_spans, rand_spans, merged_spans);
    }
    
    println!();
}
