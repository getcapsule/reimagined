use std::{collections::VecDeque, sync::Arc};

#[derive(Debug)]
pub struct Queue<T> {
    entries: Arc<VecDeque<T>>,
    current: usize,
}

impl<T> FromIterator<T> for Queue<T> {
    fn from_iter<I: IntoIterator<Item = T>>(entries: I) -> Self {
        Self {
            entries: Arc::new(entries.into_iter().collect()),
            current: 0,
        }
    }
}

impl<T> Queue<T> {
    pub fn entries(&self) -> &VecDeque<T> {
        &self.entries
    }
    pub fn push_back(&mut self, item: T)
    where
        T: Clone,
    {
        Arc::make_mut(&mut self.entries).push_back(item);
    }
    pub fn index(&self) -> usize {
        self.current
    }
    pub fn current(&self) -> Option<&T> {
        self.entries.get(self.current)
    }
    pub fn select(&mut self, index: usize) -> bool {
        if index >= self.entries.len() {
            return false;
        }
        self.current = index;
        true
    }
    pub fn remaining(&self) -> impl Iterator<Item = T> + use<T>
    where
        T: Clone,
    {
        let entries = Arc::clone(&self.entries);
        (self.current..entries.len()).map(move |index| entries[index].clone())
    }
}
