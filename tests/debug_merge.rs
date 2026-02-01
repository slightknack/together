//! Debug test for merge_divergent_edits failure

use together::crdt::rga::Rga;
use together::key::KeyPair;

fn main() {
    // Recreate the minimal failing case from proptest output:
    // base_ops = [Insert { pos_pct: 0.0, content: [117, 101, 121] }]
    // This creates "uey" at position 0

    let user1 = KeyPair::generate();
    let user2 = KeyPair::generate();

    // Create shared base with user1
    let mut base = Rga::new();
    base.insert(&user1.key_pub, 0, &[117, 101, 121]); // "uey"
    println!("Base: {:?}", base.to_string());

    // Clone for both users
    let mut rga1 = base.clone();
    let mut rga2 = base.clone();

    // Apply edit1 operations with user1
    // Edit1 has multiple inserts and deletes - let's simplify
    // The key operations that might cause issues:
    // - Insert at various positions
    // - Deletes that span across original content

    // Let's try a simpler case: both insert at position 1
    rga1.insert(&user1.key_pub, 1, b"AAA");
    rga2.insert(&user2.key_pub, 1, b"BBB");

    println!("rga1 after edit: {:?}", rga1.to_string());
    println!("rga2 after edit: {:?}", rga2.to_string());

    // Merge both ways
    let mut m12 = rga1.clone();
    m12.merge(&rga2);

    let mut m21 = rga2.clone();
    m21.merge(&rga1);

    println!("m12 (rga1.merge(rga2)): {:?}", m12.to_string());
    println!("m21 (rga2.merge(rga1)): {:?}", m21.to_string());

    assert_eq!(m12.to_string(), m21.to_string(), "Merge should be commutative!");
    println!("SUCCESS: merge is commutative");
}
