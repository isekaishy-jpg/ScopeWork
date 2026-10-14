# Build Scopework

## Requirements

- 64-bit Windows with the Visual Studio C++ build tools and Windows SDK.
- Rust 1.95 or newer, with the MSVC toolchain and `rustfmt`.
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

The Windows workflow builds the executables and MSI and retains build artifacts. It does not run tests. Keep installer binaries and research data out of Git.

## Release preparation

Increment the three-part version in `Cargo.toml`, update `CHANGELOG.md`, build the MSI, and attach the MSI and its checksum to a GitHub release. Record exactly which build, runtime, install and upgrade checks were performed. Use a draft or prerelease while release qualification is incomplete.

Keep the installer UpgradeCode stable. Do not rename the executable filenames or CLI command casually; scripts and installed shortcuts may depend on them.
