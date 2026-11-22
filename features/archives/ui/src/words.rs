use std::path::{Path, PathBuf};

use nova_files_api::Tally;
use nova_files_archives_api::{Compress, Extract};
use nova_files_shell::island::job_words::{Doing, Failed, JobWords};
use nova_files_shell::island::words::{name_of, with_answers};

pub struct ExtractWords;

impl JobWords<Extract> for ExtractWords {
    fn doing(&self, _: &Extract) -> Doing {
        Doing {
            verb: "Extracting",
            after_items: "",
        }
    }

    fn sources<'a>(&self, job: &'a Extract) -> &'a [PathBuf] {
        std::slice::from_ref(&job.archive)
    }

    fn done(&self, job: &Extract, _: Option<&Path>, tally: Tally) -> Option<String> {
        Some(with_answers(
            vec![format!("Extracted {}", name_of(&job.archive))],
            tally,
        ))
    }

    fn failed(&self, _: &Extract) -> Failed {
        Failed {
            verb: "extract",
            names_item: true,
        }
    }
}

pub struct CompressWords;

impl JobWords<Compress> for CompressWords {
    fn doing(&self, _: &Compress) -> Doing {
        Doing {
            verb: "Compressing",
            after_items: "",
        }
    }

    fn sources<'a>(&self, job: &'a Compress) -> &'a [PathBuf] {
        &job.sources
    }

    fn done(&self, job: &Compress, _: Option<&Path>, _: Tally) -> Option<String> {
        Some(format!("Made {}", job.name.to_string_lossy()))
    }

    fn failed(&self, _: &Compress) -> Failed {
        Failed {
            verb: "compress",
            names_item: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use nova_files_archives_api::{ExtractTo, Packing};

    use super::*;

    fn tally(replaced: u64, skipped: u64) -> Tally {
        Tally { replaced, skipped }
    }

    #[test]
    fn an_extract_names_its_archive_and_the_answers_given() {
        let extract = Extract {
            archive: "/home/a/photos.zip".into(),
            into: "/home/a".into(),
            to: ExtractTo::Here,
        };
        let done = |tally| ExtractWords.done(&extract, None, tally);
        assert_eq!(done(tally(0, 0)).as_deref(), Some("Extracted photos.zip"));
        assert_eq!(
            done(tally(0, 1)).as_deref(),
            Some("Extracted photos.zip, skipped 1")
        );
        assert_eq!(
            done(tally(1, 0)).as_deref(),
            Some("Extracted photos.zip, replaced 1")
        );
        assert_eq!(
            ExtractWords.sources(&extract),
            [PathBuf::from("/home/a/photos.zip")]
        );
    }

    #[test]
    fn a_compress_names_what_it_made() {
        let compress = Compress {
            sources: vec!["/home/a/notes".into()],
            into: "/home/a".into(),
            name: "notes.zip".into(),
            packing: Packing::ZipDeflate,
            level: 6,
            locked: false,
        };
        assert_eq!(
            CompressWords
                .done(&compress, None, Tally::default())
                .as_deref(),
            Some("Made notes.zip")
        );
        assert_eq!(CompressWords.doing(&compress).verb, "Compressing");
    }
}
