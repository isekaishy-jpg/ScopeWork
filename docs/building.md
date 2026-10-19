# Build Scopework

## Requirements

- 64-bit Windows with the Visual Studio C++ build tools and Windows SDK.
- Rust 1.98.0 (pinned by `rust-toolchain.toml`), with the MSVC toolchain, `rustfmt` and Clippy.
- .NET 8 SDK or newer for MSI packaging.

```powershell
git clone https://github.com/isekaishy-jpg/ScopeWork.git
cd ScopeWork
cargo build --release --locked --bins
cargo run --release --bin research-dashboard
```

The outputs are `target/release/research-dashboard.exe` and `target/release/research.exe`. The package and executable names remain compatible with earlier releases; the displayed product name is Scopework.

## MSI

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File installer/build.ps1
```

The execution-policy override applies only to that process. The script builds both executables, obtains WiX 6.0.2 and its matching UI extension into ignored local directories, and produces `dist/Scopework-<product version>-x64.msi` plus a SHA-256 checksum file. Each run is a numbered package; see Versions and build numbers below.

Close the app if rebuilding the executable it is currently running from. Research lives outside the build directory.

## Repository layout

| Path | Purpose |
| --- | --- |
| `src/main.rs` | Desktop dashboard |
| `src/research.rs` | Research library and report editor |
| `src/bin/research.rs` | Agent CLI |
| `src/paths.rs` | Shared research-home resolution |
| `installer/` | MSI components, wizard and build script |
| `assets/` | Application icon sources and exports |
| `docs/` | User and contributor guides |

The Windows workflow builds the executables and MSI and retains build artifacts. It runs focused regression tests and warning-denying Clippy checks. Keep installer binaries and research data out of Git.

## Versions and build numbers

Scopework follows the Solarity convention ([proposal](proposal-versioning.md)).

**Versions.** `Cargo.toml` is the version source:

| Stage | Cargo version | Product version |
| --- | --- | --- |
| Alpha | `0.13.0-alpha` | `0.13.0a` |
| Beta | `0.13.0-beta` | `0.13.0b` |
| Release candidate | `0.13.0-rc` | `0.13.0rc` |
| Stable | `0.13.0` | `0.13.0s` |

**Build numbers.** A build number increases once per packaged build and is never reused. Display: `Scopework 0.13.0a (Build 000001)`.
- **Reserving.** `installer/build.ps1` reserves the next number before compiling, under an exclusive package lock in Git's common directory, which linked worktrees share. It raises both the tracked `BUILD_NUMBER` and a high-water mark there, so switching branches, deleting `target` or a failed build never reuses a number; failures leave gaps. Commit the updated `BUILD_NUMBER` with the package, and push it before another checkout packages.
- **Not reserving.** Ordinary `cargo build`, tests and lint checks never reserve. They embed the committed number, which is `0` before the first package.
- **CI.** CI runs `build.ps1 -UseCommittedNumber`, which builds the committed number without reserving.
- **Checking identity.** `research --version` and `research --build-info` report the product version, the build number and the source revision, with a dirty flag. The About dialog shows the same line.

**The MSI version** is `major.minor.build`, because Windows Installer compares only three numeric fields. Build numbers only grow, so every package upgrades the previous one across stages and versions. The installer's comments field shows the product version and build.

**A version change** is a release decision. Before one, review every commit since the previous version change and write `docs/releases/<product version>.md`. It holds grouped notes, the comparison range, validation, known limitations and a ledger of every commit, including the version and package commits. Link it from the version's `CHANGELOG.md` section.

## Release preparation

Change the version in `Cargo.toml` (see above), write the release notes, and turn `## Unreleased` into `## <product version> - <date>` in `CHANGELOG.md`. Then run `installer/build.ps1` to reserve the build number and package it, and commit `BUILD_NUMBER`. Pushing a tag `v<product version>` (for example `v0.13.0a`) runs `.github/workflows/release.yml`: it checks that the tag matches the product version and that a build number is committed, repeats the format, test and Clippy checks, builds the MSI with the committed number and creates a **draft** GitHub release with the MSI, its checksum and that version's changelog section. Review the draft and publish it by hand; the workflow never publishes. The workflow can also be started manually for an existing tag. Record exactly which build, runtime, install and upgrade checks were performed. Use a draft or prerelease while release qualification is incomplete.

Keep the installer UpgradeCode stable. Do not rename the executable filenames or CLI command casually; scripts and installed shortcuts may depend on them.


## Background runtime and scale qualification

Solworker is pinned to Git commit `273b45b04f8c30d5fcc6c017bdaad4eab9ff8cde`. Cargo resolves its `vendor/micropool` package from that same Git checkout; no absolute developer path or separately copied source is required. Commit `Cargo.lock` and use `--locked` in CI and packaging. Solcache, used for the report hash cache, is pinned to Git commit `63105fecf152f4ad4e74c90fbeef24572726e0dc` the same way. A clean build needs Git/network access to those pinned repositories unless Cargo's cache was populated beforehand.

The import runner uses two CPU workers and two file-provider workers, with 256-record checkpoints. These are per-runner limits, not a global host budget. The GUI keeps at most one snapshot rebuild, one report load, one mutation and one coalesced browser query in flight; it never joins them on the input thread. A bounded owner phase publishes snapshots.

`python scripts/qualify_import.py --exe PATH-TO-RESEARCH --name UNIQUE-RUN` creates a temporary 16-function smoke fixture, records elapsed import time, then removes the entire fixture on success or failure. `--count` accepts only 1–100; large on-disk qualification inventories are prohibited. Only one compact JSON measurement survives under `target/qualification/UNIQUE-RUN.json`. This is a functional smoke measurement, not evidence of whole-inventory performance. It never uses the installed research home.

The package build gives Cargo/WiX an owned temporary directory, places WiX intermediates there, and removes it in `finally`, restoring the caller's TEMP/TMP values. The output is the MSI and checksum; no `.wixpdb` is generated. Cargo build products and the reusable `.tools`/`.wix` toolchains remain intentional build caches. Never clean unrelated temporary folders, downloaded installers, Windows Installer caches, or research directories as part of packaging.
