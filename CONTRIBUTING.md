# Contributing to mpp-linux

Thanks for helping. These rules keep the history clean and the project safe for
people's pens, digitizers and private data.

## 1. Branches

- `main` is always in a working state. Never commit broken code to it.
- Do work on a short-lived branch named after the change type:

  | Prefix | Use for | Example |
  |---|---|---|
  | `feat/` | new functionality | `feat/pen-report-parser` |
  | `fix/` | bug fixes | `fix/battery-scale` |
  | `docs/` | documentation only | `docs/protocol-pairing` |
  | `refactor/` | code changes that don't change behaviour | `refactor/hidraw-io` |
  | `test/` | tests only | `test/uhid-replay` |
  | `chore/` | tooling, CI, dependencies | `chore/bump-zbus` |

- Keep branches small. One topic per branch.
- Merge into `main` with a **fast-forward or rebase** (no merge commits).

## 2. Commit messages

We use [Conventional Commits](https://www.conventionalcommits.org/).

```
<type>(<scope>): <summary>

<body: what changed and why — wrap at 72 characters>

<footer: e.g. "Fixes #12" or "BREAKING CHANGE: ...">
```

- **type**: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `perf`, `ci`
- **scope**: the crate or area, e.g. `core`, `daemon`, `ctl`, `settings`,
  `pairing`, `haptics`, `container`, `nix`, `ci`, `docs`
- **summary**: imperative mood ("add", not "added"), lower case, no full stop,
  max ~60 characters.
- **body**: required for anything non-trivial. Explain *why*, not just *what*.

Good:

```
fix(core): read pen battery as 0..255, not a percentage

The digitizer reports Battery Strength with logical maximum 255. We
showed raw values as percent, so a full pen read 255 %. Scale by the
descriptor's logical range instead.

Fixes #31
```

Bad: `fixed stuff`, `WIP`, `update files`, `feat: lots of changes`.

## 3. What makes a good commit

- **One logical change per commit.** A reviewer should be able to understand it
  on its own. Don't mix formatting, refactoring and features.
- **Every commit builds and passes tests.** Run before committing:

  ```sh
  cargo fmt --all
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  container/build.sh          # glibc + musl, see README
  ```

- Bug fixes include a test that fails without the fix.
- Update the relevant file in `docs/` in the same commit as the change it
  documents.

## 4. Never commit

- **Real captures**: hid-recorder dumps, btmon/btsnoop files, Windows ETW traces,
  screenshots or notes with Bluetooth addresses, link keys, pen serial numbers or
  anything else from someone's machine. Keep them in the git-ignored `captures/`
  folder. Commit only synthetic or hand-scrubbed fixtures under `fixtures/` and
  say in the commit body how they were scrubbed.
- **Windows binaries** (drivers, installers, MSIX packages, Windows images) or
  anything extracted from them. They are not ours to redistribute. List them in
  `docs/artifacts.tsv` and fetch them with `tools/fetch-artifacts.sh`.
- **Decompiled or disassembled code** (§6).
- Secrets, tokens, API keys, build output, large binaries (> 1 MB).

Check `git status` and `git diff --staged` before every commit.
`tools/privacy/check.sh` runs in CI and fails on tracked captures,
Windows binaries, Bluetooth addresses, private IP addresses and key material.
Install it as a pre-commit hook:

```sh
printf '#!/bin/sh\nexec tools/privacy/check.sh --staged\n' > .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

Put your own hostnames, serial numbers and names in `.privacy-denylist` (one regex
per line). That file is git-ignored, so the terms it protects are never published.

## 5. License and evidence

mpp-linux is licensed under the **GNU AGPL-3.0-or-later** (see `LICENSE`). By
contributing you agree your contribution is released under that license. Code
reused from other projects keeps its header, is named in the commit body and is
allowed only from AGPL/GPL-3.0(+), MIT, BSD, Apache-2.0, MPL-2.0 or LGPL-3.0
sources. Kernel patches drafted under `upstream/linux/` are GPL-2.0, as the kernel
requires.

Every protocol fact in `docs/protocol/` carries a source and one label:

| Label | Meaning |
|---|---|
| `VERIFIED-HW` | Seen on real hardware; cites the capture and frame/offset |
| `STATIC` | Read from a shipped binary; cites file, SHA-256 and function/offset |
| `DOC` | Public documentation; cites the URL |
| `HYPOTHESIS` | Our own guess |

Observed behaviour wins over static analysis, which wins over documentation.

## 6. Safety and clean-room rules

- **Never flash, update or erase firmware** on a pen or a digitizer, and never
  send output/SET_FEATURE reports that `docs/protocol/` hasn't classified as safe.
  Unknown vendor reports are read-only until classified.
- **Never pair, unpair or reset Bluetooth devices** from tests or tools without
  the user asking for it.
- **Clean room:** protocol docs describe behaviour in our own words with
  citations. Implementation code is written from `docs/protocol/`, never from
  decompiler output, which must not enter the tree in any form.
- Tests that need real hardware are `#[ignore]`d and run with
  `cargo test -- --ignored`. Everything else runs on recorded, scrubbed fixtures
  (uhid replay) so CI never touches a device.

## 7. Platforms

**glibc and musl are equal targets, on x86_64 and aarch64.** A change lands only
when all four combinations pass CI (`.github/workflows/ci.yml`). Don't depend on
glibc-only behaviour (`dlopen` tricks, NSS, `gnu` target assumptions) or on a
specific architecture's layout; read device data with explicit endianness.

## 8. Pull requests and versioning

- Title in Conventional Commit form; description says what, why and how it was
  tested (and on which hardware, if any). CI must be green.
- [Semantic Versioning](https://semver.org/); tags `vX.Y.Z` on `main`; notable
  changes go in `CHANGELOG.md` under `Unreleased`.

## 9. Accessibility

**mpp-linux targets WCAG 2.2 Level AA for every user-facing surface, enforced in
CI.** The standard and checklist are in [docs/a11y.md](docs/a11y.md).
Non-negotiables: no information by colour alone; full keyboard operation with
visible focus; accessible names on every control; 4.5:1 contrast in both themes;
layouts survive 200 % text scale; respect reduce-motion; 24×24 px minimum targets;
every haptic cue has a visual or audible equivalent. A UI change merges only with
its automated AT-SPI2 check present and green.
