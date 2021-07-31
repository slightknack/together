mod rope;
pub use rope::*;

fn main() {
    let mut doc = rope::Doc {
        contents: vec![],
    };

    doc.automerge_insert(rope::Entry {
        item: "hello",
        id:   rope::Id(0),
        seq:  0,
        parent: None,
    });

    doc.automerge_insert(rope::Entry {
        item: " world",
        id:   rope::Id(1),
        seq:  1,
        parent: Some(rope::Id(0)),
    });

    doc.automerge_insert(rope::Entry {
        item: "nice work: ",
        id:   rope::Id(2),
        seq:  2,
        parent: None,
    });

    doc.automerge_insert(rope::Entry {
        item: "John! ",
        id:   rope::Id(3),
        seq:  2,
        parent: Some(rope::Id(2)),
    });

    println!("{:#?}", doc);

    println!("entry: {}", std::mem::size_of::<Option<rope::Id>>());
}
