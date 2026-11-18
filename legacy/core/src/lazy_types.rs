use std::ffi::OsStr;
use std::sync::{Arc, OnceLock};

use nova_files_api::{FileTypes, IconKind};
use nova_files_disk::ports::Mimes;

use crate::mime::MimeTypes;

#[derive(Debug)]
pub struct LazyTypes(pub Arc<OnceLock<MimeTypes>>);

impl LazyTypes {
    fn types(&self) -> &MimeTypes {
        self.0.get_or_init(MimeTypes::from_env)
    }
}

impl FileTypes for LazyTypes {
    fn of(&self, name: &OsStr) -> Option<&str> {
        self.types().of(name)
    }

    fn is_a(&self, mime: &str, wanted: &str) -> bool {
        self.types().is_a(mime, wanted)
    }
}

impl Mimes for LazyTypes {
    fn ancestry(&self, mime: &str) -> Vec<String> {
        self.types().ancestry(mime)
    }

    fn name(&self, mime: &str) -> Option<String> {
        self.types().name(mime)
    }

    fn icon_names(&self, mime: &str) -> Vec<String> {
        self.types().icon_names(mime)
    }

    fn icon_kind(&self, mime: &str) -> IconKind {
        self.types().icon_kind(mime)
    }
}
