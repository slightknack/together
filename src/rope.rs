#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Id(pub u16);

#[derive(Debug)]
pub struct Entry<T> {
    pub item: T,
    // 16 + 32 + 16 = 64
    pub id: Id,
    pub seq: u32,
    pub parent: Option<Id>,
}

#[derive(Debug)]
pub struct Doc<T> {
    pub contents: Vec<Entry<T>>,
}

impl<T> Doc<T> {
    pub fn find_item(&self, parent: Option<Id>) -> usize {
        if let Some(id) = parent {
            self.contents.iter().position(|e| e.id == id).unwrap() + 1
        } else {
            0
        }
    }

    pub fn automerge_insert(&mut self, entry: Entry<T>) {
        let parent_index = self.find_item(entry.parent);

        let mut index = self.contents.len();
        for i in parent_index..self.contents.len() {
            let old_entry = &self.contents[i];
            if entry.seq > old_entry.seq { index = i; break; }
            let old_parent_index = self.find_item(old_entry.parent);

            if old_parent_index < parent_index
            || (old_parent_index == parent_index
                && (entry.seq == old_entry.seq)
                && entry.id < old_entry.id
            ) { index = i; break; }
        }

        self.contents.insert(index, entry);
    }
}
