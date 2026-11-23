use super::matching::{self, Found, MatchKind, TYPO_MIN};
use crate::fold;

const PATH_SEPARATOR: u8 = b'/';

pub struct Terms {
    terms: Vec<Term>,
}

struct Term {
    folders: Vec<Vec<u8>>,
    name: Vec<u8>,
    letters: u32,
    along_path: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Met {
    pub by_folders: usize,
    pub loosely: bool,
    pub found: Found,
}

impl Terms {
    pub fn parse(query: &str) -> Self {
        let mut folded = Vec::new();
        let terms = query
            .split_whitespace()
            .filter_map(|word| {
                fold::fold_into(word.as_bytes(), &mut folded);
                Term::parse(&folded)
            })
            .collect();
        Self { terms }
    }

    pub const fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    pub const fn len(&self) -> usize {
        self.terms.len()
    }

    pub fn enter(&self, reach: &mut [u8], folder: &[u8]) {
        for (term, reached) in self.terms.iter().zip(reach) {
            let wanted = if term.along_path {
                term.folders.get(usize::from(*reached))
            } else {
                (*reached == 0).then_some(&term.name)
            };
            if wanted.is_some_and(|piece| matching::contains(folder, piece)) {
                *reached = reached.saturating_add(1);
            }
        }
    }

    pub fn could_meet(&self, reach: &[u8], letters: u32, loose: bool) -> bool {
        self.terms.iter().zip(reach).all(|(term, &reached)| {
            let may_miss = u32::from(loose && term.name.len() >= TYPO_MIN);
            let in_name = (term.letters & !letters).count_ones() <= may_miss;
            if term.along_path {
                in_name && usize::from(reached) == term.folders.len()
            } else {
                in_name || reached > 0
            }
        })
    }

    pub fn meet(
        &self,
        reach: &[u8],
        folded: &[u8],
        loose: bool,
        starts_word: impl Fn(usize) -> bool,
    ) -> Option<Met> {
        let mut met = Met {
            by_folders: 0,
            loosely: false,
            found: Found {
                kind: MatchKind::Exact,
                cost: 0,
            },
        };
        let mut anchored = false;
        for (term, &reached) in self.terms.iter().zip(reach) {
            if term.along_path && usize::from(reached) != term.folders.len() {
                return None;
            }
            anchored |= term.along_path;
            if term.name.is_empty() {
                continue;
            }
            let found = if let Some(found) = matching::exact(folded, &term.name, &starts_word) {
                found
            } else if !term.along_path && reached > 0 {
                met.by_folders += 1;
                continue;
            } else if loose {
                met.loosely = true;
                matching::loose(folded, &term.name)?
            } else {
                return None;
            };
            anchored = true;
            met.found.kind = met.found.kind.max(found.kind);
            met.found.cost += found.cost;
        }
        anchored.then_some(met)
    }
}

impl Term {
    fn parse(folded: &[u8]) -> Option<Self> {
        let mut pieces: Vec<&[u8]> = folded.split(|&byte| byte == PATH_SEPARATOR).collect();
        let along_path = pieces.len() > 1;
        let name = pieces.pop().unwrap_or_default();
        let folders: Vec<Vec<u8>> = pieces
            .into_iter()
            .filter(|piece| !piece.is_empty())
            .take(usize::from(u8::MAX))
            .map(<[u8]>::to_vec)
            .collect();
        (!name.is_empty() || !folders.is_empty()).then(|| Self {
            folders,
            name: name.to_vec(),
            letters: fold::letters(name),
            along_path,
        })
    }
}
