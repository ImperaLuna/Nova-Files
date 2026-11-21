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

## Layout

- `framework/`: what every feature plugs into (base types, messages, jobs,
  the list on screen, the window shell and the window). It never uses a
  feature.
- `features/<name>/{api,core,ui}`: one folder per feature. `api` holds its
  messages and types, `core` the work on disk, `ui` what the window shows.
  A feature uses the framework and other features' `api` crates, never
  another feature's `core` or `ui`.
- `fronts/`: other ways in (the D-Bus service); `api` crates only.
- `helpers/`: separate programs and libraries one feature's core uses.
- `app/`: wiring only, and the whole-window tests.

## Engineering principles

- One source of truth, no hard-coded values. Every value (a color, size,
  radius, opacity, gradient stop, easing, duration, timeout, path, command
  name) is a named constant defined once, in the module that owns it, and
  referenced everywhere else. Design values are theme tokens, even when one
  component uses them (`radius::ROW`, not `14.0`); a component only refers
  to tokens or derives ratios from them. When logic shows up twice, move it
  into a shared function or component before adding a third copy.
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

## Comments

Code carries no comments. Names say what the code does; when they can't,
reshape the code. A rule or limit the code relies on ("never", "only", "at
most") becomes a type, a test or a lint, not a sentence. What a change
leaves unsolved goes under "Known gaps" in the PR body. Only these stay:

- `// SAFETY:` on an unsafe block.
- Docs (`///`, `//!`) in `nova-files-api` and each feature's `api` crate
  (`features/*/api`): the contract other apps and features build on.
- A licence header (`// SPDX-License-Identifier:`).
- A fact forced by the kernel, a filesystem, a spec or a dependency, true
  today, in one or two plain lines that link its source (`https://`). A
  workaround for an upstream bug links the issue and says how to remove it.

A lint's reason goes in `#[expect(lint, reason = "...")]`. When a change
makes a kept comment false, fix it in the same change. `just comments`
checks Rust files (rules and their tests in `tools/ast-grep/comments`).

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
  each struct and enum) and cap their size. Exception: files people edit by
  hand (`settings.toml`, `theme.toml`) are read key by key; an unknown key
  or bad value is logged and skipped, and the other keys still apply.
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
`just clippy nova-files-jobs`); most take an optional crate. Run
Every commit you push has passed `just check` (`nix flake check`) locally.
It records its pass for that commit, and the pre-push hook refuses a commit
without one. CI runs it again as the safety net.

Before asking Codex for review, review the whole PR diff yourself with the
`/code-review` skill (high effort) and fix what it finds: Codex is the last
check, not the first reviewer. For crash recovery, races and file
operations, list the edge cases (a crash between steps, a link swapped in,
two processes at once) and test them first. Ask Codex once per PR. Fix only
findings that crash, lose or damage data, or that a user hits in normal
use; answer the rest in their thread and resolve it, without a push.

Scratch output (results, logs, traces, scripts, dumps) goes in
`.claude/<ticket>/` inside this checkout, never in the home folder. Work in
the one checkout of each repo; don't clone extra copies.

Every change goes through a pull request. Push with `git push`, never
through GitHub's file API, so the repo's pre-push hook runs (`nix develop`
turns it on; elsewhere run `git config core.hooksPath tools/git-hooks`); it
checks the passes above and runs rustfmt and Clippy on the crates the push
changes. A
stacked pull request (base isn't main) changes code only: the crate
folders (`app/`, `framework/`, `features/`, `fronts/`, `helpers/`),
`data/`, `Cargo.toml` and `Cargo.lock`, without Markdown or
lint configs.
Everything else goes in its own pull request off main.

## Rules and their checks

Each rule above and what enforces it. "Review" means nothing does yet.

| Rule | Enforced by |
|---|---|
| No comments outside the keep list; a kept fact is at most two lines | `just comments` (ast-grep rules in `tools/ast-grep/comments`) |
| No hard-coded numbers | ast-grep `no-bare-number` (`just numbers`) |
| Logic shows up twice: share it | PMD CPD (`just duplicates`) |
| Small files and functions | `just lengths` (800 lines); Clippy `too_many_lines`, `too_many_arguments`, `excessive_nesting` |
| No `unwrap`, `expect`, `panic` or `todo` outside tests | Clippy restriction lints |
| No silent error swallowing (`.ok()` on I/O) | Review |
| No polling loops | Clippy `disallowed-methods` (`std::thread::sleep`) |
| The window never touches the disk or starts processes | `framework/window/clippy.toml` bans; `just clippy-config` keeps it in step with the root |
| A crate depends only on what its `Cargo.toml` allows | `just deps`; cargo-deny for licences and sources |
| A crate uses only what its folder allows (see Layout) | `just deps`, which names the broken rule |
| Unsafe code only where allowed, each block with `// SAFETY:` | rustc `unsafe_code`; Clippy `undocumented_unsafe_blocks` |
| `#[expect]` with a reason, never `#[allow]` | Clippy `allow_attributes`, `allow_attributes_without_reason` |
| Config and state files reject unknown fields | Review |
| No shell strings; argv lists only | Review |
| Every wait on another process or the network has a timeout | Review |
| A bug fix comes with a test that fails without it | Review |
| No explaining text in UI | Review |
| A stacked pull request changes code only | `tools/ci-stack-guard.sh` in CI |
| Formatting | rustfmt (`just fmt`), nixfmt |

When a rule marked Review is broken twice, fix it at the highest level that
works: the architecture, then a type, then a lint whose message names the
fix, then a test; a line here is the last resort. A new check comes with
tests that fail on a real past mistake. An exception sits on the line it
excuses (`#[expect(lint, reason = "...")]`, or `// ast-grep-ignore: <rule>`
under a comment line) and names its reason, an end date and the user's
approval.

## Review guidelines

- Check the rules above. Report only bugs that crash, lose or damage data,
  or that a user hits in normal use, such as UI-thread blocking, lost
  updates, missed file changes, data loss in file operations and waits
  without a timeout.
- A race that needs another program to act inside a narrow window goes in
  the summary as a note, not as a finding.
- Each finding names the steps that trigger it.
- Don't flag ratios derived from theme values.
