use std::path::PathBuf;

use nova_files_api::{Failure, Operation, Outcome};
use nova_files_jobs::identity::Identity;
use nova_files_jobs::mover::Mover;
use nova_files_jobs::running::Running;
use nova_files_jobs::undo::{landed, trash_again};
use nova_files_jobs::{Ended, JobKind};

use crate::inside;
use crate::{Archiver, Asked, Taking, compress, extract};

pub struct Archives<M> {
    pub archiver: Option<Archiver>,
    pub files: M,
}

impl<M: Mover + Send + Sync + 'static> JobKind for Archives<M> {
    type Op = Operation;
    type Undo = Vec<(PathBuf, Identity)>;

    fn claims(&self, operation: &Operation) -> bool {
        match operation {
            Operation::Copy { sources, .. } => copied_out(sources).is_some(),
            Operation::Extract { .. } | Operation::Compress { .. } => true,
            _ => false,
        }
    }

    fn run(
        &self,
        running: &mut Running,
        operation: &Operation,
        undo: Option<&Self::Undo>,
    ) -> Ended {
        if let Some(made) = undo {
            return trash_again(running, made.clone());
        }
        let archiver = self.archiver.as_ref();
        if let Operation::Copy { sources, into } = operation
            && let Some((archive, picked)) = copied_out(sources)
        {
            return extract(
                running,
                (archiver, &self.files),
                &archive,
                into,
                Taking::Picked(&picked),
            );
        }
        match operation {
            Operation::Extract { archive, into, to } => extract(
                running,
                (archiver, &self.files),
                archive,
                into,
                Taking::All(*to),
            ),
            Operation::Compress {
                sources,
                into,
                name,
                packing,
                level,
                locked,
            } => {
                let asked = Asked {
                    sources,
                    into,
                    name,
                    packing: *packing,
                    level: *level,
                    locked: *locked,
                };
                compress(running, archiver, &asked)
            }
            _ => (
                Outcome::Failed {
                    path: PathBuf::new(),
                    reason: Failure::Other,
                },
                Vec::new(),
            ),
        }
    }

    fn undo(&self, running: &mut Running, _: &Operation, _: &Ended) -> Option<Self::Undo> {
        let made: Self::Undo = landed(running)
            .into_iter()
            .map(|(to, _, made)| (to, made))
            .collect();
        (!made.is_empty()).then_some(made)
    }

    fn first_path(&self, operation: &Operation) -> PathBuf {
        match operation {
            Operation::Copy { sources, into } if copied_out(sources).is_some() => {
                sources.first().unwrap_or(into).clone()
            }
            Operation::Extract { archive, .. } => archive.clone(),
            Operation::Compress { sources, into, .. } => sources.first().unwrap_or(into).clone(),
            _ => PathBuf::new(),
        }
    }
}

fn copied_out(sources: &[PathBuf]) -> Option<(PathBuf, Vec<PathBuf>)> {
    let mut found = sources.iter().map(|source| {
        source
            .symlink_metadata()
            .is_err()
            .then(|| inside::within(source))
            .flatten()
    });
    let first = found.next()??;
    let mut picked = vec![first.inner];
    for within in found {
        let within = within.filter(|within| within.archive == first.archive)?;
        picked.push(within.inner);
    }
    picked
        .iter()
        .all(|inner| inner.file_name().is_some())
        .then_some((first.archive, picked))
}

#[cfg(test)]
mod tests {
    use nova_files_api::{ExtractTo, Packing};
    use nova_files_jobs::mover::Moved;

    use super::*;

    struct NoFiles;

    impl Mover for NoFiles {
        fn move_pairs(&self, _: &mut Running, _: &[(PathBuf, PathBuf)]) -> Moved {
            (Ok(()), Vec::new())
        }

        fn delete(&self, _: &mut Running, _: &[PathBuf]) -> Moved {
            (Ok(()), Vec::new())
        }
    }

    fn compress(sources: Vec<PathBuf>) -> Operation {
        Operation::Compress {
            sources,
            into: "/into".into(),
            name: "a.zip".into(),
            packing: Packing::ZipDeflate,
            level: 5,
            locked: false,
        }
    }

    #[test]
    fn archives_claim_extracting_and_compressing_and_start_at_the_archive_or_first_source() {
        let archives = Archives {
            archiver: None,
            files: NoFiles,
        };
        let extract = Operation::Extract {
            archive: "/a.zip".into(),
            into: "/into".into(),
            to: ExtractTo::Here,
        };
        let copy = Operation::Copy {
            sources: vec!["/s".into()],
            into: "/into".into(),
        };

        assert!(archives.claims(&extract) && archives.claims(&compress(Vec::new())));
        assert!(!archives.claims(&copy));
        assert_eq!(archives.first_path(&extract), PathBuf::from("/a.zip"));
        assert_eq!(
            archives.first_path(&compress(vec!["/s".into()])),
            PathBuf::from("/s")
        );
        assert_eq!(
            archives.first_path(&compress(Vec::new())),
            PathBuf::from("/into")
        );
        assert_eq!(archives.first_path(&copy), PathBuf::new());
    }
}
