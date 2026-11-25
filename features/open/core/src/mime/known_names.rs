use super::{Glob, MimeTypes};

const KNOWN_NAME_WEIGHT: u32 = 100;

const KNOWN_NAMES: [(&str, &str); 14] = [
    ("Dockerfile", "text/x-dockerfile"),
    ("Containerfile", "text/x-dockerfile"),
    ("go.mod", "text/x-go-mod"),
    ("go.work", "text/x-go-mod"),
    ("go.sum", "text/x-go-sum"),
    ("go.work.sum", "text/x-go-sum"),
    ("Cargo.lock", "application/toml"),
    ("poetry.lock", "application/toml"),
    ("uv.lock", "application/toml"),
    ("flake.lock", "application/json"),
    ("justfile", "text/x-justfile"),
    ("Gemfile", "application/x-ruby"),
    ("Rakefile", "application/x-ruby"),
    ("Vagrantfile", "application/x-ruby"),
];

impl MimeTypes {
    pub(super) fn add_known_names(&mut self, mut order: usize) {
        for (pattern, mime) in KNOWN_NAMES {
            self.add(Glob {
                weight: KNOWN_NAME_WEIGHT,
                mime: mime.to_owned(),
                pattern: pattern.to_owned(),
                case_sensitive: false,
                order,
            });
            order += 1;
        }
    }
}
