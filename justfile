# One command per job. Run them inside `nix develop`.
#
# Recipes that take `crate` run on the whole workspace when it's left out:
# `just test` or `just test nova-files-core`. `just check` runs everything
# CI runs; use it before every push.

# List the recipes.
default:
    @just --list

# Everything CI runs: `nix flake check`, then mutation tests on the
# lines changed since main (they need git history, which the Nix sandbox
# doesn't have, so they can't run inside it).
check:
    nix flake check
    just mutants

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
mutants base="origin/main" crate="" *flags:
    mkdir -p target
    git diff {{ base }}... > target/mutants.diff
    cargo mutants --test-tool nextest --in-diff target/mutants.diff {{ if crate == "" { "--workspace" } else { "--package " + crate } }} {{ flags }}
