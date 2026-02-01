// Comparative benchmark suite for RGA implementations
//
// Benchmarks all 5 implementations:
// - YjsRga: YATA algorithm (yjs-style)
// - DiamondRga: B-tree based (diamond-types style)
// - ColaRga: Anchor-based with Lamport timestamps
// - JsonJoyRga: Dual-tree with splay optimization
// - LoroRga: Fugue algorithm

use criterion::{
    black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput,
};
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;

use together::crdt::rga_trait::Rga;
use together::crdt::yjs::YjsRga;
use together::crdt::diamond::DiamondRga;
use together::crdt::cola::ColaRga;
use together::crdt::json_joy::JsonJoyRga;
use together::crdt::loro::LoroRga;
use together::crdt::rga_optimized::OptimizedRga;
use together::key::KeyPair;

// Helper to create users with deterministic keys for reproducibility
fn make_users(count: usize, seed: u64) -> Vec<KeyPair> {
    // For benchmarks, we just generate random keys
    // The seed doesn't affect key generation but we use it for other randomness
    (0..count).map(|_| KeyPair::generate()).collect()
}

// =============================================================================
// Benchmark Helpers
// =============================================================================

/// Insert content character-by-character at sequential positions (forward typing)
fn sequential_forward<R: Rga<UserId = together::key::KeyPub>>(
    rga: &mut R,
    user: &together::key::KeyPub,
    content: &[u8],
) {
    for (i, byte) in content.iter().enumerate() {
        rga.insert(user, i as u64, &[*byte]);
    }
}

/// Insert content character-by-character in reverse (backspace pattern)
fn sequential_backward<R: Rga<UserId = together::key::KeyPub>>(
    rga: &mut R,
    user: &together::key::KeyPub,
    content: &[u8],
) {
    // First insert all content
    rga.insert(user, 0, content);
    // Then delete from the end one by one (simulating backspace)
    for _ in 0..content.len() {
        let len = rga.len();
        if len > 0 {
            rga.delete(len - 1, 1);
        }
    }
}

/// Insert content at random positions
fn random_inserts<R: Rga<UserId = together::key::KeyPub>>(
    rga: &mut R,
    user: &together::key::KeyPub,
    content: &[u8],
    rng: &mut StdRng,
) {
    for byte in content.iter() {
        let len = rga.len();
        let pos = if len == 0 { 0 } else { rng.gen_range(0..=len) };
        rga.insert(user, pos, &[*byte]);
    }
}

/// Delete at random positions
fn random_deletes<R: Rga<UserId = together::key::KeyPub>>(
    rga: &mut R,
    count: usize,
    rng: &mut StdRng,
) {
    for _ in 0..count {
        let len = rga.len();
        if len == 0 {
            break;
        }
        let pos = rng.gen_range(0..len);
        rga.delete(pos, 1);
    }
}

/// Mixed insert and delete operations
fn mixed_operations<R: Rga<UserId = together::key::KeyPub>>(
    rga: &mut R,
    user: &together::key::KeyPub,
    ops: usize,
    rng: &mut StdRng,
) {
    for _ in 0..ops {
        let len = rga.len();
        // 70% insert, 30% delete (typical editing pattern)
        if len == 0 || rng.gen_bool(0.7) {
            let pos = if len == 0 { 0 } else { rng.gen_range(0..=len) };
            let byte = rng.gen_range(b'a'..=b'z');
            rga.insert(user, pos, &[byte]);
        } else {
            let pos = rng.gen_range(0..len);
            rga.delete(pos, 1);
        }
    }
}

// =============================================================================
// Sequential Typing (Forward) Benchmarks
// =============================================================================

fn bench_sequential_forward(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequential_forward");
    
    let sizes = [100, 1000, 10000];
    
    for size in sizes {
        let content: Vec<u8> = (0..size).map(|i| b'a' + (i % 26) as u8).collect();
        group.throughput(Throughput::Elements(size as u64));
        
        group.bench_with_input(BenchmarkId::new("YjsRga", size), &content, |b, content| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = YjsRga::new();
                sequential_forward(&mut rga, &user.key_pub, content);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("DiamondRga", size), &content, |b, content| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = DiamondRga::new();
                sequential_forward(&mut rga, &user.key_pub, content);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("ColaRga", size), &content, |b, content| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = ColaRga::new();
                sequential_forward(&mut rga, &user.key_pub, content);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("JsonJoyRga", size), &content, |b, content| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = JsonJoyRga::new();
                sequential_forward(&mut rga, &user.key_pub, content);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("LoroRga", size), &content, |b, content| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = LoroRga::new();
                sequential_forward(&mut rga, &user.key_pub, content);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("OptimizedRga", size), &content, |b, content| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = OptimizedRga::new();
                sequential_forward(&mut rga, &user.key_pub, content);
                black_box(rga.len())
            });
        });
    }
    
    group.finish();
}

// =============================================================================
// Sequential Typing (Backward/Backspace) Benchmarks
// =============================================================================

fn bench_sequential_backward(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequential_backward");
    
    let sizes = [100, 1000];
    
    for size in sizes {
        let content: Vec<u8> = (0..size).map(|i| b'a' + (i % 26) as u8).collect();
        group.throughput(Throughput::Elements(size as u64));
        
        group.bench_with_input(BenchmarkId::new("YjsRga", size), &content, |b, content| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = YjsRga::new();
                sequential_backward(&mut rga, &user.key_pub, content);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("DiamondRga", size), &content, |b, content| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = DiamondRga::new();
                sequential_backward(&mut rga, &user.key_pub, content);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("ColaRga", size), &content, |b, content| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = ColaRga::new();
                sequential_backward(&mut rga, &user.key_pub, content);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("JsonJoyRga", size), &content, |b, content| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = JsonJoyRga::new();
                sequential_backward(&mut rga, &user.key_pub, content);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("LoroRga", size), &content, |b, content| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = LoroRga::new();
                sequential_backward(&mut rga, &user.key_pub, content);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("OptimizedRga", size), &content, |b, content| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = OptimizedRga::new();
                sequential_backward(&mut rga, &user.key_pub, content);
                black_box(rga.len())
            });
        });
    }
    
    group.finish();
}

// =============================================================================
// Random Inserts Benchmarks
// =============================================================================

fn bench_random_inserts(c: &mut Criterion) {
    let mut group = c.benchmark_group("random_inserts");
    
    let sizes = [100, 1000, 5000];
    
    for size in sizes {
        let content: Vec<u8> = (0..size).map(|i| b'a' + (i % 26) as u8).collect();
        group.throughput(Throughput::Elements(size as u64));
        
        group.bench_with_input(BenchmarkId::new("YjsRga", size), &content, |b, content| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = YjsRga::new();
                let mut rng = StdRng::seed_from_u64(42);
                random_inserts(&mut rga, &user.key_pub, content, &mut rng);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("DiamondRga", size), &content, |b, content| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = DiamondRga::new();
                let mut rng = StdRng::seed_from_u64(42);
                random_inserts(&mut rga, &user.key_pub, content, &mut rng);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("ColaRga", size), &content, |b, content| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = ColaRga::new();
                let mut rng = StdRng::seed_from_u64(42);
                random_inserts(&mut rga, &user.key_pub, content, &mut rng);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("JsonJoyRga", size), &content, |b, content| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = JsonJoyRga::new();
                let mut rng = StdRng::seed_from_u64(42);
                random_inserts(&mut rga, &user.key_pub, content, &mut rng);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("LoroRga", size), &content, |b, content| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = LoroRga::new();
                let mut rng = StdRng::seed_from_u64(42);
                random_inserts(&mut rga, &user.key_pub, content, &mut rng);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("OptimizedRga", size), &content, |b, content| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = OptimizedRga::new();
                let mut rng = StdRng::seed_from_u64(42);
                random_inserts(&mut rga, &user.key_pub, content, &mut rng);
                black_box(rga.len())
            });
        });
    }
    
    group.finish();
}

// =============================================================================
// Random Deletes Benchmarks
// =============================================================================

fn bench_random_deletes(c: &mut Criterion) {
    let mut group = c.benchmark_group("random_deletes");
    
    let sizes = [100, 1000, 5000];
    
    for size in sizes {
        group.throughput(Throughput::Elements(size as u64));
        
        group.bench_with_input(BenchmarkId::new("YjsRga", size), &size, |b, &size| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = YjsRga::new();
                // First build up the document
                let content: Vec<u8> = (0..size).map(|i| b'a' + (i % 26) as u8).collect();
                rga.insert(&user.key_pub, 0, &content);
                // Then delete randomly
                let mut rng = StdRng::seed_from_u64(42);
                random_deletes(&mut rga, size, &mut rng);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("DiamondRga", size), &size, |b, &size| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = DiamondRga::new();
                let content: Vec<u8> = (0..size).map(|i| b'a' + (i % 26) as u8).collect();
                rga.insert(&user.key_pub, 0, &content);
                let mut rng = StdRng::seed_from_u64(42);
                random_deletes(&mut rga, size, &mut rng);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("ColaRga", size), &size, |b, &size| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = ColaRga::new();
                let content: Vec<u8> = (0..size).map(|i| b'a' + (i % 26) as u8).collect();
                rga.insert(&user.key_pub, 0, &content);
                let mut rng = StdRng::seed_from_u64(42);
                random_deletes(&mut rga, size, &mut rng);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("JsonJoyRga", size), &size, |b, &size| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = JsonJoyRga::new();
                let content: Vec<u8> = (0..size).map(|i| b'a' + (i % 26) as u8).collect();
                rga.insert(&user.key_pub, 0, &content);
                let mut rng = StdRng::seed_from_u64(42);
                random_deletes(&mut rga, size, &mut rng);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("LoroRga", size), &size, |b, &size| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = LoroRga::new();
                let content: Vec<u8> = (0..size).map(|i| b'a' + (i % 26) as u8).collect();
                rga.insert(&user.key_pub, 0, &content);
                let mut rng = StdRng::seed_from_u64(42);
                random_deletes(&mut rga, size, &mut rng);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("OptimizedRga", size), &size, |b, &size| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = OptimizedRga::new();
                let content: Vec<u8> = (0..size).map(|i| b'a' + (i % 26) as u8).collect();
                rga.insert(&user.key_pub, 0, &content);
                let mut rng = StdRng::seed_from_u64(42);
                random_deletes(&mut rga, size, &mut rng);
                black_box(rga.len())
            });
        });
    }
    
    group.finish();
}

// =============================================================================
// Mixed Insert/Delete Benchmarks
// =============================================================================

fn bench_mixed_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("mixed_operations");
    
    let sizes = [100, 1000, 5000];
    
    for size in sizes {
        group.throughput(Throughput::Elements(size as u64));
        
        group.bench_with_input(BenchmarkId::new("YjsRga", size), &size, |b, &size| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = YjsRga::new();
                let mut rng = StdRng::seed_from_u64(42);
                mixed_operations(&mut rga, &user.key_pub, size, &mut rng);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("DiamondRga", size), &size, |b, &size| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = DiamondRga::new();
                let mut rng = StdRng::seed_from_u64(42);
                mixed_operations(&mut rga, &user.key_pub, size, &mut rng);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("ColaRga", size), &size, |b, &size| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = ColaRga::new();
                let mut rng = StdRng::seed_from_u64(42);
                mixed_operations(&mut rga, &user.key_pub, size, &mut rng);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("JsonJoyRga", size), &size, |b, &size| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = JsonJoyRga::new();
                let mut rng = StdRng::seed_from_u64(42);
                mixed_operations(&mut rga, &user.key_pub, size, &mut rng);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("LoroRga", size), &size, |b, &size| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = LoroRga::new();
                let mut rng = StdRng::seed_from_u64(42);
                mixed_operations(&mut rga, &user.key_pub, size, &mut rng);
                black_box(rga.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("OptimizedRga", size), &size, |b, &size| {
            let user = KeyPair::generate();
            b.iter(|| {
                let mut rga = OptimizedRga::new();
                let mut rng = StdRng::seed_from_u64(42);
                mixed_operations(&mut rga, &user.key_pub, size, &mut rng);
                black_box(rga.len())
            });
        });
    }
    
    group.finish();
}

// =============================================================================
// Large Document Merge Benchmarks
// =============================================================================

fn bench_large_merge(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_merge");
    
    // Merge two documents of equal size
    let sizes = [1000, 5000, 10000];
    
    for size in sizes {
        let content_a: Vec<u8> = (0..size).map(|i| b'A' + (i % 26) as u8).collect();
        let content_b: Vec<u8> = (0..size).map(|i| b'a' + (i % 26) as u8).collect();
        
        group.throughput(Throughput::Elements((size * 2) as u64));
        
        group.bench_with_input(BenchmarkId::new("YjsRga", size), &size, |b, _| {
            let user1 = KeyPair::generate();
            let user2 = KeyPair::generate();
            b.iter(|| {
                let mut rga_a = YjsRga::new();
                let mut rga_b = YjsRga::new();
                rga_a.insert(&user1.key_pub, 0, &content_a);
                rga_b.insert(&user2.key_pub, 0, &content_b);
                rga_a.merge(&rga_b);
                black_box(rga_a.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("DiamondRga", size), &size, |b, _| {
            let user1 = KeyPair::generate();
            let user2 = KeyPair::generate();
            b.iter(|| {
                let mut rga_a = DiamondRga::new();
                let mut rga_b = DiamondRga::new();
                rga_a.insert(&user1.key_pub, 0, &content_a);
                rga_b.insert(&user2.key_pub, 0, &content_b);
                rga_a.merge(&rga_b);
                black_box(rga_a.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("ColaRga", size), &size, |b, _| {
            let user1 = KeyPair::generate();
            let user2 = KeyPair::generate();
            b.iter(|| {
                let mut rga_a = ColaRga::new();
                let mut rga_b = ColaRga::new();
                rga_a.insert(&user1.key_pub, 0, &content_a);
                rga_b.insert(&user2.key_pub, 0, &content_b);
                rga_a.merge(&rga_b);
                black_box(rga_a.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("JsonJoyRga", size), &size, |b, _| {
            let user1 = KeyPair::generate();
            let user2 = KeyPair::generate();
            b.iter(|| {
                let mut rga_a = JsonJoyRga::new();
                let mut rga_b = JsonJoyRga::new();
                rga_a.insert(&user1.key_pub, 0, &content_a);
                rga_b.insert(&user2.key_pub, 0, &content_b);
                rga_a.merge(&rga_b);
                black_box(rga_a.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("LoroRga", size), &size, |b, _| {
            let user1 = KeyPair::generate();
            let user2 = KeyPair::generate();
            b.iter(|| {
                let mut rga_a = LoroRga::new();
                let mut rga_b = LoroRga::new();
                rga_a.insert(&user1.key_pub, 0, &content_a);
                rga_b.insert(&user2.key_pub, 0, &content_b);
                rga_a.merge(&rga_b);
                black_box(rga_a.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("OptimizedRga", size), &size, |b, _| {
            let user1 = KeyPair::generate();
            let user2 = KeyPair::generate();
            b.iter(|| {
                let mut rga_a = OptimizedRga::new();
                let mut rga_b = OptimizedRga::new();
                rga_a.insert(&user1.key_pub, 0, &content_a);
                rga_b.insert(&user2.key_pub, 0, &content_b);
                rga_a.merge(&rga_b);
                black_box(rga_a.len())
            });
        });
    }
    
    group.finish();
}

// =============================================================================
// Many Small Merges Benchmarks
// =============================================================================

fn bench_many_small_merges(c: &mut Criterion) {
    let mut group = c.benchmark_group("many_small_merges");
    
    // Many users each contributing small edits
    let user_counts = [5, 10, 20];
    let edits_per_user = 100;
    
    for num_users in user_counts {
        group.throughput(Throughput::Elements((num_users * edits_per_user) as u64));
        
        group.bench_with_input(BenchmarkId::new("YjsRga", num_users), &num_users, |b, &num_users| {
            b.iter(|| {
                let users: Vec<_> = (0..num_users).map(|_| KeyPair::generate()).collect();
                let mut rgas: Vec<_> = (0..num_users).map(|_| YjsRga::new()).collect();
                
                // Each user types their edits
                for (i, (user, rga)) in users.iter().zip(rgas.iter_mut()).enumerate() {
                    let content: Vec<u8> = (0..edits_per_user)
                        .map(|j| b'a' + ((i * edits_per_user + j) % 26) as u8)
                        .collect();
                    for (j, byte) in content.iter().enumerate() {
                        rga.insert(&user.key_pub, j as u64, &[*byte]);
                    }
                }
                
                // Merge all into the first
                let mut merged = rgas.remove(0);
                for rga in &rgas {
                    merged.merge(rga);
                }
                black_box(merged.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("DiamondRga", num_users), &num_users, |b, &num_users| {
            b.iter(|| {
                let users: Vec<_> = (0..num_users).map(|_| KeyPair::generate()).collect();
                let mut rgas: Vec<_> = (0..num_users).map(|_| DiamondRga::new()).collect();
                
                for (i, (user, rga)) in users.iter().zip(rgas.iter_mut()).enumerate() {
                    let content: Vec<u8> = (0..edits_per_user)
                        .map(|j| b'a' + ((i * edits_per_user + j) % 26) as u8)
                        .collect();
                    for (j, byte) in content.iter().enumerate() {
                        rga.insert(&user.key_pub, j as u64, &[*byte]);
                    }
                }
                
                let mut merged = rgas.remove(0);
                for rga in &rgas {
                    merged.merge(rga);
                }
                black_box(merged.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("ColaRga", num_users), &num_users, |b, &num_users| {
            b.iter(|| {
                let users: Vec<_> = (0..num_users).map(|_| KeyPair::generate()).collect();
                let mut rgas: Vec<_> = (0..num_users).map(|_| ColaRga::new()).collect();
                
                for (i, (user, rga)) in users.iter().zip(rgas.iter_mut()).enumerate() {
                    let content: Vec<u8> = (0..edits_per_user)
                        .map(|j| b'a' + ((i * edits_per_user + j) % 26) as u8)
                        .collect();
                    for (j, byte) in content.iter().enumerate() {
                        rga.insert(&user.key_pub, j as u64, &[*byte]);
                    }
                }
                
                let mut merged = rgas.remove(0);
                for rga in &rgas {
                    merged.merge(rga);
                }
                black_box(merged.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("JsonJoyRga", num_users), &num_users, |b, &num_users| {
            b.iter(|| {
                let users: Vec<_> = (0..num_users).map(|_| KeyPair::generate()).collect();
                let mut rgas: Vec<_> = (0..num_users).map(|_| JsonJoyRga::new()).collect();
                
                for (i, (user, rga)) in users.iter().zip(rgas.iter_mut()).enumerate() {
                    let content: Vec<u8> = (0..edits_per_user)
                        .map(|j| b'a' + ((i * edits_per_user + j) % 26) as u8)
                        .collect();
                    for (j, byte) in content.iter().enumerate() {
                        rga.insert(&user.key_pub, j as u64, &[*byte]);
                    }
                }
                
                let mut merged = rgas.remove(0);
                for rga in &rgas {
                    merged.merge(rga);
                }
                black_box(merged.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("LoroRga", num_users), &num_users, |b, &num_users| {
            b.iter(|| {
                let users: Vec<_> = (0..num_users).map(|_| KeyPair::generate()).collect();
                let mut rgas: Vec<_> = (0..num_users).map(|_| LoroRga::new()).collect();
                
                for (i, (user, rga)) in users.iter().zip(rgas.iter_mut()).enumerate() {
                    let content: Vec<u8> = (0..edits_per_user)
                        .map(|j| b'a' + ((i * edits_per_user + j) % 26) as u8)
                        .collect();
                    for (j, byte) in content.iter().enumerate() {
                        rga.insert(&user.key_pub, j as u64, &[*byte]);
                    }
                }
                
                let mut merged = rgas.remove(0);
                for rga in &rgas {
                    merged.merge(rga);
                }
                black_box(merged.len())
            });
        });
        
        group.bench_with_input(BenchmarkId::new("OptimizedRga", num_users), &num_users, |b, &num_users| {
            b.iter(|| {
                let users: Vec<_> = (0..num_users).map(|_| KeyPair::generate()).collect();
                let mut rgas: Vec<_> = (0..num_users).map(|_| OptimizedRga::new()).collect();
                
                for (i, (user, rga)) in users.iter().zip(rgas.iter_mut()).enumerate() {
                    let content: Vec<u8> = (0..edits_per_user)
                        .map(|j| b'a' + ((i * edits_per_user + j) % 26) as u8)
                        .collect();
                    for (j, byte) in content.iter().enumerate() {
                        rga.insert(&user.key_pub, j as u64, &[*byte]);
                    }
                }
                
                let mut merged = rgas.remove(0);
                for rga in &rgas {
                    merged.merge(rga);
                }
                black_box(merged.len())
            });
        });
    }
    
    group.finish();
}

// =============================================================================
// Criterion Configuration
// =============================================================================

criterion_group!(
    benches,
    bench_sequential_forward,
    bench_sequential_backward,
    bench_random_inserts,
    bench_random_deletes,
    bench_mixed_operations,
    bench_large_merge,
    bench_many_small_merges,
);

criterion_main!(benches);
