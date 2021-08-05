use crate::collection::Collection;

pub enum IndexTree<T> {
    Node {
        size: usize,
        children: Vec<Self>,
    },
    Leaf(T),
}

impl<T> Collection for IndexTree<T> {
    fn len(&self) -> usize {
        match self {
            IndexTree::Leaf(_) => 1,
            IndexTree::Node { size, .. } => *size,
        }
    }

    /// So proud of this bad boy, splits a `IndexTree` in place.
    /// Here's how it works:
    /// 1. Keep track of the current node, starting at the outermost
    /// 2. Look up the index of the child relative to the current node
    /// 3. Split off everything after the index, and collect the orphans
    /// 4. Join the orphans together into a new tree.
    /// Of course, I employ a few optimizations to keep things fresh.
    fn split(mut self, mut index: usize) -> (Self, Self) {
        let mut current = &mut self;
        let mut orphans = vec![];

        while let IndexTree::Node {
            ref mut size,
            ref mut children,
        } = current {
            let (before, i) = Self::child_index(children, index);
            for orphan in children.split_off(i + 1).into_iter().rev() {
                orphans.push(orphan);
            }
            *size = before + children[i].len();
            current = &mut children[i];
            index -= before;
        }

        orphans.reverse();
        (self, Self::new_from_children(orphans))
    }

    fn append(&mut self, other: Self) {
        let new_size = self.len() + other.len();
        let moved_self = std::mem::replace(self, Self::new());

        if let IndexTree::Node { size, children } = self {
            *size = new_size;
            children.push(moved_self);
            children.push(other);
        }
    }
}

impl<T> IndexTree<T> {
    pub fn new() -> Self {
        IndexTree::Node { size: 0, children: vec![] }
    }

    pub fn new_from_children(children: Vec<Self>) -> Self {
        let mut size = 0;
        for child in children.iter() { size += child.len() }
        IndexTree::Node { size, children }
    }

    /// Returns the index of the child containing the index,
    /// as well as the total size of all children before that child,
    /// in the order: `(before, index)`.
    fn child_index(children: &[Self], index: usize) -> (usize, usize) {
        let mut before = 0;
        let mut i = 0;
        for _ in 0..children.len() {
            let next = children[i].len();
            if before + next > index { break; }
            before += next;
            i += 1;
        }
        (before, i)
    }

    pub fn get(&self, index: usize) -> &T {
        match self {
            IndexTree::Leaf(item) => item,
            IndexTree::Node { children, .. } => {
                let (before, i) = Self::child_index(children, index);
                children[i].get(index - before)
            }
        }
    }

    pub fn insert(&mut self, item: T, index: usize) {
        let moved_self = std::mem::replace(self, Self::new());
        let (mut left, right) = moved_self.split(index);
        left.append(IndexTree::Leaf(item));
        left.append(right);
        *self = left;
    }
}
