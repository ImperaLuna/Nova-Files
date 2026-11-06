# Nova Files

A fast, polished file manager for Linux, written in Rust on GPUI and
packaged with a Nix flake. It is a standalone app: it must drop into any
Linux desktop, not only Nova, one distro or one compositor. Priorities, in
order: speed, then a polished, soft and rounded UI.

`README.md` is written by hand by the maintainer. Don't edit it.

This file is the only agent instruction file (Claude Code and Codex both read
it). Don't add a `CLAUDE.md` or `CLAUDE.local.md`: Claude Code then skips
`AGENTS.md`.

## Architecture

One program, `nova-files`, runs as one process with two halves: the **core**
does the work (index, search, folder listing, watching, jobs, settings,
theme) and the UI draws. In **service mode** only the core runs, with no
window and no GPU; D-Bus starts it when a launcher searches. Opening a window
joins the running process (single instance). The process exits 3 minutes
after the last window, search or job ends, and nothing starts at login.

Crates, each in `crates/<name>`, and the only outside crates each may use:

- `nova-files`: start-up, single instance, service mode or window, wiring the
  core to the UI. No logic of its own. Uses our crates only.
- `nova-files-api`: the commands the UI sends, the events it gets back, and
  the shared data types. Plain data, no I/O. Uses `std` only.
- `nova-files-core`: search, folder listing, watching, jobs and their
  journal, settings and theme. `rustix`, `serde`, `toml`.
- `nova-files-index`: the on-disk index format. `memmap2`, `rustix`.
- `nova-files-ui`: every window and view. `gpui`, `gpui-component`.
- `nova-files-dbus`: the public D-Bus interface and launcher adapters. `zbus`.
- `nova-files-thumbnailer`: a sandboxed helper program that thumbnails one
  batch of files, then exits. `image`, `landlock`.

Rules that follow from it:

- The UI depends on `nova-files-api` alone and reaches the core only through
  its commands and events. It never reads files, not even for previews.
- The UI never waits on the core or the disk. Results arrive as events, so a
  slow or hung mount never freezes a frame.
- Folder listings travel in batches (about 1,000 entries per event), and
  large data is shared through `Arc`, not copied.
- Own the core, borrow the parsers, sandbox what we borrow. Code that parses
  file contents (images, PDFs, video, fonts, archives) runs only inside the
  thumbnailer or another sandboxed helper. Adding an outside crate means
  adding it to the list above in the same PR, with the reason.
- Work runs on threads with channels. Our crates stay free of tokio and
  other async runtimes; `zbus` runs on its own executor.
- The public D-Bus interface is the one stable surface, so changes to it stay
  backward compatible. Everything else, `nova-files-api` included, may change
  in any release.
- The index holds names and metadata, never file contents. It's a cache: on
  a version mismatch or damage, rebuild it. Rows store parent rows, and full
  paths are rebuilt from them.
- Settings live in one file the core owns and pushes live to every window
  and launcher. Window state (size, tabs, last folder) belongs to the UI;
  the core saves it as an opaque blob.
- The core resolves the theme in order: the theme file in the config folder,
  then the settings portal's light/dark mode and accent colour, then the
  built-in defaults.
- The core and UI share a process, so jobs keep a journal: a copy or move
  cut off by a crash resumes on the next start.

## UI rules

- No explaining text in UI: if a UI needs an extra line to describe what a
  feature does or what a state means, the UX is wrong; fix the design
  instead. Never add text that repeats what an icon, number or state already
  shows (e.g. "Headphones · connected" in a connected list, "needs charging
  soon" next to 5%, "battery" next to a battery icon). Titles are short
  enough to fit; the key value (like a percentage) is always visible.
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

- One source of truth. A value used in more than one place (a color, size,
  radius, duration, timeout, path, command name) is defined once and
  referenced everywhere else: design values in the theme module, Rust
  constants in the module that owns them. Never copy a literal into a
  second file.
- Each module and file opens with a comment saying what it does and how.
  Read it before changing that code and keep it true in the same change.
- No magic numbers or strings. Give values a named constant that says what
  they mean (`radius::ROW`, not `14.0`). Every design value (colors,
  opacities, gradient stops, easing) lives in the theme, even when one
  component uses it; a component only refers to theme values or derives
  ratios from them.
- Don't repeat logic. When the same code shows up twice, move it into a
  shared component or function before adding a third copy.
- Names say what something is: no `tmp`, `data2` or single letters outside
  short loops.
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
- Leave code cleaner than you found it, but keep refactors in their own
  commit.
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
- Network calls use HTTPS only, have a timeout and a size cap on the answer,
  and back off on errors.
- Secrets (tokens, passwords) live in types that don't derive `Debug`,
  `Display` or `Serialize`, so they can't reach a log or a saved file by
  accident.
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

Work inside `nix develop`: `cargo build`, `cargo test`, `cargo clippy` and
`cargo fmt`. `nix build` builds the package.

Every change goes through a pull request.

## Review guidelines

- Check the rules above, and look hardest for UI-thread blocking, races,
  lost updates, missed file changes, data loss in file operations, waits
  without a timeout, secrets that could reach a log, and a crate using
  anything outside its list in Architecture.
- Each finding names the steps that trigger it, and says when they're rare.
- Don't flag ratios derived from theme values.
