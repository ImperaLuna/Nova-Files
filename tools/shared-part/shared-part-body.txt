Closes FIL-26

## Why
GitHub refuses agent merges inside a stack, so this lands the whole reviewed FIL-26 stack (#324, #325, #326, #327, all green) as one PR off main.

| Risk | Blast radius |
|---|---|
| 🟢 low | archives feature only; same code as the four reviewed PRs |

## What changed
- Same tree as #327's head: #324 password island, #325 Compress as… island, #326 extract all from inside an archive, #327 cancelled listing and locked tarball fixes.

## Evidence

## Shared part
- framework/window: the password prompt. It needs the island's text field, which only the window holds today.
- framework/shell: a password look for the island's field, which any feature can ask for.

## Evidence
All four stacked PRs passed nix flake check and the stack guard.
