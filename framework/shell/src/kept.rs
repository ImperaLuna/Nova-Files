use gpui::Context;
use nova_files_api::WindowState;

#[derive(Debug, Default)]
pub struct Kept {
    loaded: WindowState,
    changed: WindowState,
}

impl Kept {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.changed.get(key).or_else(|| self.loaded.get(key))
    }

    pub fn set(&mut self, key: &str, value: &str) {
        self.changed.set(key, value);
    }

    pub fn arrived(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        self.loaded = WindowState::parse(bytes);
        cx.notify();
    }

    pub fn take_changed(&mut self) -> Option<Vec<u8>> {
        let changed = std::mem::take(&mut self.changed);
        if changed.is_empty() {
            return None;
        }
        self.loaded = std::mem::take(&mut self.loaded).merged(changed.clone());
        Some(changed.to_bytes())
    }
}
