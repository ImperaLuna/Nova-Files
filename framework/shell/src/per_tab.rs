use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(pub u64);

#[derive(Debug)]
pub struct PerTab<T> {
    tabs: HashMap<TabId, T>,
}

impl<T> Default for PerTab<T> {
    fn default() -> Self {
        Self {
            tabs: HashMap::new(),
        }
    }
}

impl<T> PerTab<T> {
    pub fn get(&self, tab: TabId) -> Option<&T> {
        self.tabs.get(&tab)
    }

    pub fn get_mut(&mut self, tab: TabId) -> Option<&mut T> {
        self.tabs.get_mut(&tab)
    }

    pub fn set(&mut self, tab: TabId, value: T) {
        self.tabs.insert(tab, value);
    }

    pub fn or_insert_with(&mut self, tab: TabId, make: impl FnOnce() -> T) -> &mut T {
        self.tabs.entry(tab).or_insert_with(make)
    }

    pub fn closed(&mut self, tab: TabId) -> Option<T> {
        self.tabs.remove(&tab)
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }
}

impl<T: Default> PerTab<T> {
    pub fn of(&mut self, tab: TabId) -> &mut T {
        self.tabs.entry(tab).or_default()
    }
}
