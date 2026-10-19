# Proposal: product versions and numbered builds

Adopts the Solarity convention (`solarityclient/docs/releasing.md`) for Scopework, as recorded in TODO.md. The user asked for this item end to end on 2026-10-18; this document holds its research, design and proposal, and the implementation follows it.

## Research

**The Solarity convention.**
- `Cargo.toml` is the version source.
- The Cargo spelling `X.Y.Z-alpha`, `-beta`, `-rc` or plain `X.Y.Z` maps to the product spelling `X.Y.Za`, `b`, `rc` or `s`.
- A separate build number increases once per packaged build and is never reused. Failed attempts leave gaps. Display: `Solarity 0.0.0a (Build 000001)`.
- The packaging script reserves the number before compiling. It writes both the tracked `BUILD_NUMBER` and a high-water mark in Git's common directory (shared by worktrees), and holds an exclusive lock there for the whole build.
- `build.rs` embeds the product version, `BUILD_NUMBER`, the Git revision and a dirty flag.
- `--version` prints the display line and `--build-info` prints the identity fields.
- A version change needs patch notes covering every commit since the previous version change.

**Scopework today.**
- **Installer:** `installer/build.ps1` takes the Cargo version as the MSI `ProductVersion` and rejects anything but numeric `X.Y.Z`. `Package.wxs` keeps one `UpgradeCode` with a plain `MajorUpgrade`, so each package must have a strictly higher `ProductVersion`, compared on its first three fields (at most 255.255.65535).
- **Workflows:** `release.yml` runs on tags `v*.*.*`, requires the tag to equal `v` plus the Cargo version, and extracts the CHANGELOG section `## <version> `. `windows.yml` also runs `build.ps1`, on every CI build.
- **Displays:** the GUI's About dialog shows `CARGO_PKG_VERSION`. `build.rs` compiles Windows resources whose product-version string defaults to the Cargo version.
- **Coordinator:** clients and coordinators compare versions with `version_key`, which splits on `.`, `-` and `+`. It would read `alpha` as 0, so it cannot order stages. The handshake already carries a `build` field, always 0 today.
- **History:** published versions run 0.1.0 to 0.8.0, tagged from v0.4.0. TODO.md records the next version, 0.13.0a, and how it was derived.

## Design

| Question | Decision | Why |
| --- | --- | --- |
| Product spelling | Solarity's mapping; Cargo `0.13.0-alpha` is `0.13.0a` | One convention across the user's projects |
| MSI `ProductVersion` | `X.Y.<build number>` | Build numbers never repeat and only grow, so every package upgrades the previous one, across stages and versions. The patch digit is not lost for users: it stays in the product version, the file name and every display. `X.Y.Z` alone would give two packages of one version the same `ProductVersion`, so the second would not upgrade the first. |
| Where the number lives | Tracked `BUILD_NUMBER`, plus a high-water mark and package lock in Git's common directory, as in Solarity | Survives clones (tracked), branch switches, worktrees and failed builds (high-water mark) |
| Who reserves | Only `installer/build.ps1` in its default mode. CI uses `-UseCommittedNumber`, which builds with the committed number without reserving. | CI runners cannot keep a sequence. A release tag builds the number its commit records. |
| Source builds | `build.rs` reads `BUILD_NUMBER` (0 if absent) and the Git revision and dirty flag, or `unknown` without Git | Ordinary `cargo build` and tests never reserve, and build anywhere |
| Identity | `research --version` prints `Scopework 0.13.0a (Build 000001)`; `research --build-info` prints JSON (product, Cargo version, build, revision, dirty). The GUI About dialog and the executables' version resources show the same line. | Agents and the installer can check exactly what runs |
| Coordinator ordering | Parse versions as (X, Y, Z, stage) with alpha < beta < rc < stable. When both sides carry a build number, the higher number is newer. | Builds are the true package order; source builds (number 0) fall back to the version |
| Tags and release notes | Tag `v` plus the product version (`v0.13.0a`), which still matches `v*.*.*`. CHANGELOG section `## 0.13.0a - <date>`. Full notes and a commit ledger in `docs/releases/<version>.md`. | Solarity's rule: every version change reviews every commit since the last one |

## Proposal

1. **`BUILD_NUMBER`** at the repository root, starting at `0` (the source state before the first numbered package).
2. **`build.rs`** generates `build_identity.rs` with the product version, build number, revision and dirty flag. It also sets the Windows resources' product-version string to the display line. It reruns when `Cargo.toml`, `BUILD_NUMBER`, `src` or the Git head or index change.
3. **`src/build_info.rs`** (library) exposes the identity: `PRODUCT`, `BUILD`, `REVISION`, `DIRTY`, `display()` and `json()`. The CLI answers `--version` and `--build-info`, and the GUI's About dialog shows `display()`.
4. **Coordinator handshake** sends the build number and compares (version, stage, build) as designed. The protocol is unchanged.
5. **`installer/build.ps1`:**
   - holds the package lock in Git's common directory and reserves the next number (unless `-UseCommittedNumber`);
   - maps the version, builds, and checks that `research.exe --build-info` reports the reserved number;
   - packages `Scopework-<product>-x64.msi` with `ProductVersion` `X.Y.<build>`.

   `windows.yml` and `release.yml` use `-UseCommittedNumber`. `release.yml` also requires a nonzero number, the tag `v<product>` and the CHANGELOG section `## <product> `.
6. **The version change itself:**
   - `Cargo.toml` becomes `0.13.0-alpha`;
   - the CHANGELOG's Unreleased section becomes `## 0.13.0a - 2026-10-18`;
   - `docs/releases/0.13.0a.md` gets the notes and the ledger of every commit since `v0.8.0`;
   - `docs/building.md` documents the procedure.
7. **The first numbered package:** a local package run reserves Build 000001, and its `BUILD_NUMBER` is committed. Publishing (pushing the tag) stays the user's step.

## Validation

- **Unit tests:** the version mapping (every stage; malformed versions refused) and the coordinator's version ordering (stages, build numbers, source builds).
- **Integration test:** `research --version` and `--build-info` agree with `Cargo.toml` and `BUILD_NUMBER`.
- **Packaging run:** reserves the next number exactly once. The executable reports it, and the MSI's `ProductVersion` is `X.Y.<build>`.
- **Gates:** the full suite, clippy `-D warnings` and fmt pass.
