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

The execution-policy override applies only to that process. The script builds both executables, obtains WiX 6.0.2 and its matching UI extension into ignored local directories, and produces `dist/Scopework-<version>-x64.msi` plus a SHA-256 checksum file. The MSI version comes from `Cargo.toml`.

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

## Release preparation

Increment the three-part version in `Cargo.toml`, update `CHANGELOG.md`, build the MSI, and attach the MSI and its checksum to a GitHub release. Record exactly which build, runtime, install and upgrade checks were performed. Use a draft or prerelease while release qualification is incomplete.

Keep the installer UpgradeCode stable. Do not rename the executable filenames or CLI command casually; scripts and installed shortcuts may depend on them.


## Background runtime and scale qualification

Solworker is pinned to Git commit `273b45b04f8c30d5fcc6c017bdaad4eab9ff8cde`. Cargo resolves its `vendor/micropool` package from that same Git checkout; no absolute developer path or separately copied source is required. Commit `Cargo.lock` and use `--locked` in CI and packaging. A clean build needs Git/network access to that pinned repository unless Cargo's cache was populated beforehand.

The import runner uses two CPU workers and two file-provider workers, with 256-record checkpoints. These are per-runner limits, not a global host budget. The GUI keeps at most one snapshot rebuild, one report load, one mutation and one coalesced browser query in flight; it never joins them on the input thread. A bounded owner phase publishes snapshots.

`python scripts/qualify_import.py --exe PATH-TO-RESEARCH --name UNIQUE-RUN` creates a synthetic 150,000-function fixture under `target/qualification`, records elapsed import time, and retains receipts and logs. It never uses the installed research home. Keep performance runs separate from CI assertions and report the reference machine and measured limitations.
