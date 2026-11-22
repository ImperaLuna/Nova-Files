use std::path::{Path, PathBuf};

use nova_files_api::{Failure, Operation, Tally};

use crate::island::job_words::{Batched, Doing, Failed, Grouped, JobWords, Words};

#[derive(Debug, PartialEq, Eq)]
pub struct Fetch {
    pub sources: Vec<PathBuf>,
    pub into: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Store {
    pub sources: Vec<PathBuf>,
    pub into: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Sweep {
    pub paths: Vec<PathBuf>,
}

struct FetchWords;

impl JobWords<Fetch> for FetchWords {
    fn doing(&self, _: &Fetch) -> Doing {
        Doing {
            verb: "Fetching",
            after_items: "",
        }
    }

    fn sources<'a>(&self, job: &'a Fetch) -> &'a [PathBuf] {
        &job.sources
    }

    fn done(&self, _: &Fetch, _: Option<&Path>, _: Tally) -> Option<String> {
        None
    }

    fn failed(&self, _: &Fetch) -> Failed {
        Failed {
            verb: "fetch",
            names_item: true,
        }
    }

    fn grouped(&self) -> Option<Grouped> {
        Some(Grouped {
            one: "fetch",
            many: "fetches",
        })
    }

    fn why(&self, _: &Fetch, reason: Failure) -> Option<&'static str> {
        Some(crate::island::words::failure(reason))
    }

    fn title(&self, job: &Fetch) -> Option<String> {
        Some(format!("Into {}", job.into.display()))
    }

    fn batched(&self) -> Option<Batched> {
        Some(Batched {
            done: "Fetched",
            done_after: "fetched",
            nothing: "Nothing fetched",
        })
    }
}

struct StoreWords;

impl JobWords<Store> for StoreWords {
    fn doing(&self, _: &Store) -> Doing {
        Doing {
            verb: "Storing",
            after_items: "",
        }
    }

    fn sources<'a>(&self, job: &'a Store) -> &'a [PathBuf] {
        &job.sources
    }

    fn done(&self, _: &Store, _: Option<&Path>, _: Tally) -> Option<String> {
        None
    }

    fn failed(&self, _: &Store) -> Failed {
        Failed {
            verb: "store",
            names_item: true,
        }
    }

    fn grouped(&self) -> Option<Grouped> {
        Some(Grouped {
            one: "store",
            many: "stores",
        })
    }

    fn batched(&self) -> Option<Batched> {
        Some(Batched {
            done: "Stored",
            done_after: "stored",
            nothing: "Nothing stored",
        })
    }
}

struct SweepWords;

impl JobWords<Sweep> for SweepWords {
    fn doing(&self, _: &Sweep) -> Doing {
        Doing {
            verb: "Sweeping",
            after_items: " away",
        }
    }

    fn sources<'a>(&self, job: &'a Sweep) -> &'a [PathBuf] {
        &job.paths
    }

    fn done(&self, _: &Sweep, _: Option<&Path>, _: Tally) -> Option<String> {
        Some("Swept".to_owned())
    }

    fn failed(&self, job: &Sweep) -> Failed {
        Failed {
            verb: "sweep",
            names_item: !job.paths.is_empty(),
        }
    }
}

pub fn words() -> Words {
    let mut words = Words::default();
    words.add(FetchWords);
    words.add(StoreWords);
    words.add(SweepWords);
    words
}

fn under(names: &[&str]) -> Vec<PathBuf> {
    names
        .iter()
        .map(|name| Path::new("/a").join(name))
        .collect()
}

pub fn fetch(names: &[&str], into: &str) -> Operation {
    Operation::feature(Fetch {
        sources: under(names),
        into: PathBuf::from(into),
    })
}

pub fn store(names: &[&str], into: &str) -> Operation {
    Operation::feature(Store {
        sources: under(names),
        into: PathBuf::from(into),
    })
}

pub fn sweep(names: &[&str]) -> Operation {
    Operation::feature(Sweep {
        paths: under(names),
    })
}
