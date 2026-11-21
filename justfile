# One command per job. Run them inside `nix develop`.
#
# Recipes that take `crate` run on the whole workspace when it's left out:
# `just test` or `just test nova-files-jobs`. `just check` runs before every
# push.

# List the recipes.
default:
    @just --list

# The checks CI runs, in the Nix sandbox.
check:
    nix flake check
    tools/local-pass.sh record check

# Build one crate or the workspace.
build crate="":
    cargo build {{ if crate == "" { "--workspace" } else { "--package " + crate } }}

# The app id and the portal's bus name, defined once in .cargo/config.toml.
app_id := `sed -n 's/^NOVA_FILES_APP_ID = "\(.*\)"$/\1/p' .cargo/config.toml`
portal_bus_name := `sed -n 's/^NOVA_FILES_PORTAL_BUS_NAME = "\(.*\)"$/\1/p' .cargo/config.toml`

# Where cargo builds, as it decides it.
target_dir := env("CARGO_TARGET_DIR", "target")

# Build a release and install it under `prefix` in a mode: launcher, basic
# or full (see data/install.sh). Works on any distro, outside Nix too.
install mode="basic" prefix=(env("HOME") / ".local"):
    cargo build --release --package nova-files --package nova-files-thumbnailer --package nova-files-archiver
    data/install.sh --mode "{{ mode }}" --prefix "{{ prefix }}" --from "{{ target_dir }}/release" \
        --app-id "{{ app_id }}" --portal-bus-name "{{ portal_bus_name }}"

# Remove what `just install` put under `prefix`, in any mode.
uninstall prefix=(env("HOME") / ".local"):
    data/install.sh --uninstall --prefix "{{ prefix }}" \
        --app-id "{{ app_id }}" --portal-bus-name "{{ portal_bus_name }}"

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

# framework/base and what framework/shell offers, against their ceilings.
framework-size:
    tools/check-framework-size.sh

# Links to the GPUI fork that point at another version than the pin.
fork-links:
    tools/check-fork-links.sh

# Bare numbers outside named constants.
numbers:
    tools/check-bare-numbers.sh

# Comments outside the keep list in AGENTS.md; `--report` only counts them.
comments *flags:
    tools/check-comments.sh {{ flags }}

# Rust files past the line limit.
lengths:
    tools/check-file-length.sh

# Copy-pasted code.
duplicates:
    tools/check-duplicates.sh

# A crate clippy.toml that dropped a root setting.
clippy-config:
    tools/check-clippy-config.sh
