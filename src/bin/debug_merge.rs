//! Debug test for merge_divergent_edits failure

use together::crdt::rga::Rga;
use together::crdt::Crdt;
use together::key::KeyPub;

fn apply_op(rga: &mut Rga, user: &KeyPub, pos_pct: f64, content: &[u8]) {
    let len = rga.len();
    let pos = if len == 0 { 0 } else { ((pos_pct * len as f64) as u64).min(len) };
    println!("  Insert {:?} at pos {} ({}% of {})", 
        std::str::from_utf8(content).unwrap_or("?"), pos, (pos_pct * 100.0) as u32, len);
    rga.insert(user, pos, content);
    println!("    -> {:?}", rga.to_string());
}

fn apply_delete(rga: &mut Rga, pos_pct: f64, del_len: u64) {
    let len = rga.len();
    if len == 0 { return; }
    let start = ((pos_pct * len as f64) as u64).min(len.saturating_sub(1));
    let actual_len = del_len.min(len - start);
    if actual_len == 0 { return; }
    println!("  Delete {} chars at pos {} ({}% of {})", actual_len, start, (pos_pct * 100.0) as u32, len);
    rga.delete(start, actual_len);
    println!("    -> {:?}", rga.to_string());
}

fn main() {
    // Use deterministic keys - user1 > user2 to trigger the bug
    let mut key1 = [0u8; 32];
    let mut key2 = [0u8; 32];
    key1[0] = 0xFF; // user1 is "larger"
    key2[0] = 0x00; // user2 is "smaller"
    let user1 = KeyPub(key1);
    let user2 = KeyPub(key2);
    
    println!("user1: {:?}", &user1.0[..4]);
    println!("user2: {:?}", &user2.0[..4]);
    println!("user1 > user2: {}", user1 > user2);

    // Reproduce the exact minimal failing case from proptest:
    // base_ops = [Insert { pos_pct: 0.0, content: [106] }, Insert { pos_pct: 0.3283727691025866, content: [100, 108, 111, 114, 117, 109] }]
    // This creates "j" then inserts "dlorum" at pos 0
    let mut base = Rga::new();
    apply_op(&mut base, &user1, 0.0, &[106]); // "j"
    apply_op(&mut base, &user1, 0.3283727691025866, &[100, 108, 111, 114, 117, 109]); // "dlorum"
    println!("\nBase: {:?}", base.to_string());

    let mut rga1 = base.clone();
    let mut rga2 = base.clone();

    println!("\n=== Edit1 (user1) ===");
    // edit1 = [Insert { pos_pct: 0.45662127675310543, content: [98, 98, 121, 101, 103, 112, 103, 105, 107, 107, 121, 115, 118] }, ...]
    apply_op(&mut rga1, &user1, 0.45662127675310543, &[98, 98, 121, 101, 103, 112, 103, 105, 107, 107, 121, 115, 118]); // "bbyegpgikkysv"
    apply_delete(&mut rga1, 0.9972065629493363, 1);
    apply_op(&mut rga1, &user1, 0.6090102387246018, &[98, 102, 109, 116, 117, 97, 99, 97, 97, 104, 120]); // "bfmtuacaahx"
    apply_op(&mut rga1, &user1, 0.1868593205578867, &[115, 101, 104, 108, 113, 114, 112]); // "sehlqrp"
    apply_op(&mut rga1, &user1, 0.8667075911797956, &[119, 119, 108, 97, 100, 119]); // "wwladw"
    apply_op(&mut rga1, &user1, 0.15780454611672634, &[111, 116, 111, 119, 101, 102, 98, 98, 98, 105, 103, 120, 107, 105, 102, 100]); // "otowefbbbigxkifd"
    apply_op(&mut rga1, &user1, 0.9393413817613839, &[99, 106, 99, 117, 98, 108, 112]); // "cjcublp"

    println!("\n=== Edit2 (user2) ===");
    // edit2 = [Insert { pos_pct: 0.3400436758275386, content: [120, 120, 120, 112, 113, 107, 106, 105, 107, 110, 104, 117, 103, 100, 102] }, ...]
    apply_op(&mut rga2, &user2, 0.3400436758275386, &[120, 120, 120, 112, 113, 107, 106, 105, 107, 110, 104, 117, 103, 100, 102]); // "xxxpqkjiknhugdf"
    apply_op(&mut rga2, &user2, 0.6115838072341023, &[105, 110, 102, 108, 101, 118, 99, 107, 99, 111, 112, 101, 114]); // "inflevckcopter" (13 chars)

    println!("\n=== Final states before merge ===");
    println!("rga1: {:?}", rga1.to_string());
    println!("rga2: {:?}", rga2.to_string());

    // Merge both ways
    let mut m12 = rga1.clone();
    m12.merge(&rga2);

    let mut m21 = rga2.clone();
    m21.merge(&rga1);

    println!("\n=== Span structure before merge ===");
    println!("--- rga1 spans ---");
    print!("{}", rga1.debug_spans());
    println!("--- rga2 spans ---");
    print!("{}", rga2.debug_spans());

    println!("\n=== After merge ===");
    println!("m12 (rga1.merge(rga2)): {:?}", m12.to_string());
    println!("m21 (rga2.merge(rga1)): {:?}", m21.to_string());

    if m12.to_string() == m21.to_string() {
        println!("\nSUCCESS: merge is commutative");
    } else {
        println!("\nFAILED: merge is NOT commutative!");
        println!("\n--- m12 spans ---");
        print!("{}", m12.debug_spans());
        println!("--- m21 spans ---");
        print!("{}", m21.debug_spans());
        // Find the differences
        let s1 = m12.to_string();
        let s2 = m21.to_string();
        for (i, (c1, c2)) in s1.chars().zip(s2.chars()).enumerate() {
            if c1 != c2 {
                println!("  Differ at {}: '{}' vs '{}'", i, c1, c2);
            }
        }
    }
}
