use std::path::PathBuf;

use nova_files_api::{Failure, Operation, Outcome};
use nova_files_jobs::mover::Mover;
use nova_files_jobs::running::Running;
use nova_files_jobs::{Ended, JobKind};

use crate::{Archiver, Asked, Taking, compress, extract};

pub struct Archives<M> {
    pub archiver: Option<Archiver>,
    pub files: M,
}

impl<M: Mover + Send + Sync + 'static> JobKind for Archives<M> {
    type Op = Operation;
    type Undo = ();

    fn claims(&self, operation: &Operation) -> bool {
        matches!(
            operation,
            Operation::Extract { .. } | Operation::Compress { .. }
        )
    }

    fn run(&self, running: &mut Running, operation: &Operation, _: Option<&()>) -> Ended {
        let archiver = self.archiver.as_ref();
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

    fn first_path(&self, operation: &Operation) -> PathBuf {
        match operation {
            Operation::Extract { archive, .. } => archive.clone(),
            Operation::Compress { sources, into, .. } => sources.first().unwrap_or(into).clone(),
            _ => PathBuf::new(),
        }
    }
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
