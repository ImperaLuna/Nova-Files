# Nova Files

A fast, polished file manager for Linux, written in Rust on GPUI and
packaged with a Nix flake. It is a standalone app: it must drop into any
Linux desktop, not only Nova, one distro or one compositor. Priorities, in
order: speed, then a polished, soft and rounded UI.

`README.md` is written by hand by the maintainer. Don't edit it.

This file is the only agent instruction file (Claude Code and Codex both read
it). Don't add a `CLAUDE.md` or `CLAUDE.local.md`: Claude Code then skips
`AGENTS.md`.

## UI rules

- No explaining text in UI: if a UI needs an extra line to describe what a
  feature does or what a state means, the UX is wrong; fix the design
  instead. Never add text that repeats what an icon, name or state already
  shows. In a file manager that means: no subtitle under a section heading
  ("Your recent files appear here"), no helper line under a toggle or field
  ("Show files whose names start with a dot"), no sentence in an empty
  folder (a quiet icon or "Empty" is enough), and no tooltip that repeats
  its button's label. Titles are short enough to fit; the key value (a
  size, a count, a progress percentage) is always visible.
- Refine approved designs; don't redesign them.
- Text from outside Nova Files (file names, file contents, metadata) is shown
  as plain text, whatever bytes it holds; a name that isn't valid UTF-8 still
  shows and still opens.

## Rules

- Check what the system offers (a trash, a portal, a mount service, a
  thumbnailer), never which desktop, distro or compositor is running.
- Memory: decode images at the size they're shown, don't keep what isn't
  visible, and render only the rows on screen in long lists.

## Engineering principles

- One source of truth, no hard-coded values. Every value (a color, size,
  radius, opacity, gradient stop, easing, duration, timeout, path, command
  name) is a named constant defined once, in the module that owns it, and
  referenced everywhere else. Design values are theme tokens, even when one
  component uses them (`radius::ROW`, not `14.0`); a component only refers
  to tokens or derives ratios from them. When logic shows up twice, move it
  into a shared function or component before adding a third copy.
- Each module and file opens with a comment saying what it does and how.
  Read it before changing that code and keep it true in the same change.
- Small, focused files and components: each does one job.
- Handle errors where they happen and show them in a useful form. No silent
  error swallowing or `unwrap()` on input that can fail (tests excepted).
  Permission denied, a vanished file or a full disk are normal cases.
- Design for the general case, then fix root causes. Edge cases (a second
  monitor, an empty folder, a folder with 100,000 files, a missing service,
  a read-only mount) should work because the design covers them, not through
  special-case branches. No special cases for one app or one file type when
  the bug is general. If a workaround is truly unavoidable (an upstream
  bug), keep it in one place with a comment linking the issue and how to
  remove it.
- Reference apps (Sigma, Dux Files, Flea, Strata and others) are for
  learning the approach only. Write Nova Files' own code; never copy theirs.

## State, threads and files

- A state change and what follows from it (saving it to disk, what the UI
  was last told) happen in one critical section, in order.
- A worker that can be stopped (a folder load, a copy, a size scan) checks
  for the stop before each side effect (a read, a write, a spawn), not only
  after it wakes. Navigating away cancels the old folder's work, and its
  late results never land in the new folder.
- Every wait on another process or the network (connect, read, write, a
  child, a D-Bus call) has a timeout.
- When several sources report the same value, the newest by its own
  timestamp wins, not the last to arrive.
- File watching: install the watches, then read, so nothing written in
  between is missed. Watch what may not exist yet or may be replaced (a
  folder, a symlink) through its parent and re-attach when that entry
  changes; watch a symlink's target too. A queue overflow (`IN_Q_OVERFLOW`)
  means lost events: rescan everything.
- Save files by writing a temp file in the same folder and renaming it over
  the old one. Create private files with their mode (`0600`) rather than
  changing it afterwards.
- File operations never lose data: a copy or move that fails partway leaves
  the source intact, and nothing is overwritten without the user choosing
  it.

## Security

- Never build shell strings. Open files and launch apps with argv lists
  (parse `.desktop` `Exec` keys properly); never `sh -c` with file names,
  config or `.desktop` data.
- Config and state files reject unknown fields (`deny_unknown_fields` on
  each struct and enum) and cap their size.
- Paths from config, `.desktop` files or other outside data must stay inside
  the folder they're meant for: no `..`, no absolute paths.
- Child processes get the environment they need, are always waited on, and
  have their output pipes drained.

## Performance

- Event-driven, no polling loops: watchers and workers push changes, and the
  UI redraws from them.
- Measure, don't guess: a change that could affect folder load time, memory
  or frame time states its before and after numbers in the PR.
- Animations run only while visible.

## Tests

Mostly component tests (one component through its real interface, with its
real internals) and integration tests (several together, like folder loading
against a real temp folder tree). Unit tests only for small, tricky pure
logic. Fake only hardware, mounts and network, with recorded real data, not
hand-written mocks.

Tests start from what a new user has (folders missing, no saved state)
unless they're about something else, and work in temp folders, never the
real home. A bug fix comes with a test that fails without it.

## Commands

Work inside `nix develop`. `just` lists one command per job (`just test`,
`just clippy nova-files-core`); most take an optional crate. Run
`just check` (`nix flake check`) before every push. Run `just mutants` once
per PR, after `just check` passes and before asking Codex for review; later
fix rounds only rerun `just check`. CI runs both. Paste the run's last line
(`N mutants tested in ...: ...`) into the PR description: CI's mutation job
fails at once without it, or when it reports missed or timed-out mutants. A mutant no test
can ever catch goes in `exclude_re` in `.cargo/mutants.toml`, with a reason,
through its own PR off main.

Every change goes through a pull request. Push with `git push`, never
through GitHub's file API, so the repo's pre-push hook runs (`nix develop`
turns it on; elsewhere run `git config core.hooksPath tools/git-hooks`); it
also runs rustfmt and Clippy on the crates the push changes. A
stacked pull request (base isn't main) changes code only: `crates/`,
`data/`, `Cargo.toml` and `Cargo.lock`, without Markdown or lint configs.
Everything else goes in its own pull request off main.

## Review guidelines

- Check the rules above, and look hardest for UI-thread blocking, races,
  lost updates, missed file changes, data loss in file operations and
  waits without a timeout.
- Each finding names the steps that trigger it, and says when they're rare.
- Don't flag ratios derived from theme values.
