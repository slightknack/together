use std::collections::BTreeMap;

pub struct User(usize);

// user, index, item
pub struct Id(User, usize, usize);

pub struct Entry {
    start:  usize,
    length: usize,
    parent: Id,
    seq:    usize,
}

pub trait Collection {
    fn len(&self) -> usize;
    fn split(self, index: usize) -> (Self, Self);
    fn append(&mut self, other: Self);
}

impl<T> Collection for Vec<T> {
    fn len(&self) -> usize { self.len() }

    fn split(self, index: usize) -> (Self, Self) {
        let end = self.split_off(index);
        (self, end)
    }

    fn append(&mut self, other: Self) {
        self.append(&mut other);
    }
}

impl Collection for String {
    fn len(&self) -> usize { self.len() }

    fn split(self, index: usize) -> (Self, Self) {
        let end = self.split_off(index);
        (self, end)
    }

    fn append(self, other: Self) {
        self.push_str(&other);
    }
}

pub struct Column<T: Collection> {
    user:     User,
    contents: T,
    entries:  Vec<Entry>
}

pub struct TreeLog<T: Collection> {
    columns: BTreeMap<User, Column<T>>,
    tree:    RangeMap<Id>,
}

pub enum RangeMap<T> {
    Node {
        size: usize,
        children: Vec<Self>,
    },
    Leaf {
        item: T,
    },
}
