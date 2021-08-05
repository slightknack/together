pub trait Collection where Self: Sized {
    fn len(&self) -> usize;
    fn split(self, index: usize) -> (Self, Self);
    fn append(&mut self, other: Self);
}

impl<T> Collection for Vec<T> {
    fn len(&self) -> usize { self.len() }

    fn split(mut self, index: usize) -> (Self, Self) {
        let end = self.split_off(index);
        (self, end)
    }

    fn append(&mut self, mut other: Self) {
        self.append(&mut other);
    }
}

impl Collection for String {
    fn len(&self) -> usize { self.len() }

    fn split(mut self, index: usize) -> (Self, Self) {
        let end = self.split_off(index);
        (self, end)
    }

    fn append(&mut self, other: Self) {
        self.push_str(&other);
    }
}
