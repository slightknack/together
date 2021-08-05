use std::collections::BTreeMap;
use crate::{
    collection::Collection,
    index_tree::IndexTree,
};

pub struct User(usize);

// user, index, item
pub struct Id(User, usize, usize);

pub struct Edit {
    start:  usize,
    length: usize,
    parent: Id,
    seq:    usize,
}

pub struct History<T: Collection> {
    user:     User,
    contents: T,
    entries:  Vec<Edit>,
}

pub struct TreeLog<T: Collection> {
    columns: BTreeMap<User, History<T>>,
    tree:    IndexTree<Id>,
}
