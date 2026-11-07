# One command per job. Run them inside `nix develop`.
#
# Recipes that take `crate` run on the whole workspace when it's left out:
# `just test` or `just test nova-files-core`. `just check` runs before every
# push, `just mutants` once per PR.

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
# Four builds at once by default; set CARGO_MUTANTS_JOBS to change it, or pass
# --in-place for one build in this tree. With no Rust lines changed there is
# nothing to mutate, so it stops before building.
mutants base="origin/main" crate="" *flags:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p target
    git diff {{ base }}... -- '*.rs' > target/mutants.diff
    if [ ! -s target/mutants.diff ]; then
        echo "No Rust changes since {{ base }}, nothing to mutate."
        exit 0
    fi
    if [[ " {{ flags }} " == *" --in-place "* ]]; then
        # In place there is one tree, so one build at a time, and
        # cargo-mutants refuses a job count.
        unset CARGO_MUTANTS_JOBS
    else
        export CARGO_MUTANTS_JOBS="${CARGO_MUTANTS_JOBS:-4}"
    fi
    cargo mutants --test-tool nextest --in-diff target/mutants.diff {{ if crate == "" { "--workspace" } else { "--package " + crate } }} {{ flags }}
