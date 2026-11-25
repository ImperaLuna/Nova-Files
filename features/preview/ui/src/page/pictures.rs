use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui::RenderImage;
use nova_files_preview_api::markdown::Block;

use crate::links::{Link, link};

pub const DRAWN_PICTURES: usize = 12;

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

    pub fn asking(&self) -> impl Iterator<Item = &PathBuf> {
        self.wanted
            .iter()
            .filter(|(target, _)| !self.drawn.contains_key(target))
            .map(|(_, path)| path)
    }

    pub fn arrived(&mut self, picture: &Path, image: &Arc<RenderImage>) -> Vec<Arc<RenderImage>> {
        let targets: Vec<Arc<str>> = self
            .wanted
            .iter()
            .filter(|(_, path)| path == picture)
            .map(|(target, _)| Arc::clone(target))
            .collect();
        if targets.is_empty() {
            return vec![Arc::clone(image)];
        }
        let drawn = Rc::make_mut(&mut self.drawn);
        targets
            .into_iter()
            .filter_map(|target| drawn.insert(target, Arc::clone(image)))
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

#[cfg(test)]
mod tests {
    use nova_files_preview_api::markdown::Item;

    use super::*;

    fn picture(target: &str) -> Block {
        Block::Picture {
            target: target.into(),
            alt: "".into(),
            link: None,
        }
    }

    #[test]
    fn local_pictures_are_wanted_once_each_from_every_depth() {
        let blocks = [
            picture("a.png"),
            Block::Quote(Arc::new([picture("art/b.png")])),
            Block::List {
                first: None,
                items: Arc::new([Item {
                    checked: None,
                    blocks: Arc::new([picture("a.png"), picture("https://x.org/c.png")]),
                }]),
            },
        ];
        let pictures = Pictures::new(&blocks, Some(Path::new("/notes")));
        let wanted: Vec<_> = pictures
            .wanted
            .iter()
            .map(|(_, path)| path.clone())
            .collect();
        assert_eq!(
            wanted,
            [
                PathBuf::from("/notes/a.png"),
                PathBuf::from("/notes/art/b.png")
            ]
        );
        assert_eq!(pictures.asking().count(), 2);
    }

    #[test]
    fn only_so_many_pictures_are_asked_for() {
        let blocks: Vec<Block> = (0..DRAWN_PICTURES * 2)
            .map(|index| picture(&format!("{index}.png")))
            .collect();
        let pictures = Pictures::new(&blocks, Some(Path::new("/notes")));
        assert_eq!(pictures.wanted.len(), DRAWN_PICTURES);
    }
}
