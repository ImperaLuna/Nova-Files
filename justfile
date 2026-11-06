# One command per job. Run them inside `nix develop`.
#
# Recipes that take `crate` run on the whole workspace when it's left out:
# `just test` or `just test nova-files-core`. `just check` runs before every
# push, `just mutants` once per PR.

export CARGO_MUTANTS_JOBS := env("CARGO_MUTANTS_JOBS", "4")

# List the recipes.
default:
    @just --list

# The checks CI runs in the Nix sandbox. CI also runs `just mutants`, which
# needs git history and so can't run inside the sandbox; run it once per PR.
check:
    nix flake check

# Build one crate or the workspace.
build crate="":
    cargo build {{ if crate == "" { "--workspace" } else { "--package " + crate } }}

# Format the code.
fmt:
    cargo fmt --all

# Lint with Clippy; warnings fail it, as in CI.
clippy crate="":
    cargo clippy {{ if crate == "" { "--workspace" } else { "--package " + crate } }} --all-targets -- --deny warnings

# Run the tests with nextest.
test crate="":
    cargo nextest run {{ if crate == "" { "--workspace" } else { "--package " + crate } }} --no-tests=warn

# Known vulnerabilities, licences, banned crates and sources.
deny:
    cargo deny check

# Dependencies a crate declares but never uses.
machete:
    cargo machete

# Dependencies a crate's Cargo.toml doesn't allow.
deps:
    tools/check-deps.sh

# Bare numbers outside named constants.
numbers:
    ast-grep scan --config tools/ast-grep/sgconfig.yml

# Rust files past the line limit.
lengths:
    tools/check-file-length.sh

# Copy-pasted code.
duplicates:
    tools/check-duplicates.sh

# A crate clippy.toml that dropped a root setting.
clippy-config:
    tools/check-clippy-config.sh

# Mutation tests on lines changed since `base`; a survivor is an untested change.
# Four builds at once by default; set CARGO_MUTANTS_JOBS to change it.
mutants base="origin/main" crate="" *flags:
    mkdir -p target
    git diff {{ base }}... > target/mutants.diff
    cargo mutants --test-tool nextest --in-diff target/mutants.diff {{ if crate == "" { "--workspace" } else { "--package " + crate } }} {{ flags }}
