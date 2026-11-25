use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui::RenderImage;
use nova_files_preview_api::markdown::Block;

use crate::links::{Link, link};

pub const DRAWN_PICTURES: usize = 24;

pub type Drawn = Rc<HashMap<Arc<str>, Arc<RenderImage>>>;

#[derive(Default)]
pub struct Pictures {
    pub wanted: Vec<(Arc<str>, PathBuf)>,
    pub drawn: Drawn,
}

impl Pictures {
    pub fn new(blocks: &[Block], folder: Option<&Path>) -> Self {
        let mut targets = Vec::new();
        gather(blocks, &mut targets);
        let mut wanted: Vec<(Arc<str>, PathBuf)> = Vec::new();
        for target in targets {
            if wanted.len() == DRAWN_PICTURES {
                break;
            }
            if wanted.iter().any(|(seen, _)| *seen == target) {
                continue;
            }
            if let Some(Link::Local(path)) = folder.and_then(|folder| link(&target, folder)) {
                wanted.push((target, path));
            }
        }
        Self {
            wanted,
            drawn: Rc::default(),
        }
    }

    pub fn carrying(mut self, before: Self) -> (Self, Vec<Arc<RenderImage>>) {
        let mut before = Rc::unwrap_or_clone(before.drawn);
        let kept: HashMap<_, _> = self
            .wanted
            .iter()
            .filter_map(|(target, _)| before.remove_entry(target))
            .collect();
        self.drawn = Rc::new(kept);
        (self, before.into_values().collect())
    }

    pub fn asking(&self) -> impl Iterator<Item = &PathBuf> {
        self.wanted
            .iter()
            .filter(|(target, _)| !self.drawn.contains_key(target))
            .map(|(_, path)| path)
    }

    pub fn arrived(&mut self, picture: &Path, image: &Arc<RenderImage>) -> Vec<Arc<RenderImage>> {
        let drawn = Rc::make_mut(&mut self.drawn);
        self.wanted
            .iter()
            .filter(|(_, path)| path == picture)
            .filter_map(|(target, _)| drawn.insert(Arc::clone(target), Arc::clone(image)))
            .filter(|replaced| !Arc::ptr_eq(replaced, image))
            .collect()
    }

    pub fn images(self) -> impl Iterator<Item = Arc<RenderImage>> {
        let mut seen: Vec<Arc<RenderImage>> = Vec::new();
        for image in Rc::unwrap_or_clone(self.drawn).into_values() {
            if !seen.iter().any(|kept| Arc::ptr_eq(kept, &image)) {
                seen.push(image);
            }
        }
        seen.into_iter()
    }
}

fn gather(blocks: &[Block], targets: &mut Vec<Arc<str>>) {
    for block in blocks {
        match block {
            Block::Picture { target, .. } => targets.push(Arc::clone(target)),
            Block::Quote(inner) | Block::Note { blocks: inner, .. } => gather(inner, targets),
            Block::List { items, .. } => {
                for item in items.iter() {
                    gather(&item.blocks, targets);
                }
            }
            _ => {}
        }
    }
}
