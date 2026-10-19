//! Windows resources plus the build identity: the product version, the reserved package build
//! number (`BUILD_NUMBER`) and the source revision. Reserving numbers belongs to
//! `installer/build.ps1`; ordinary Cargo builds and tests only read the committed number.
// The build script needs only the spelling; the crate also uses the ordering.
#[allow(dead_code)]
#[path = "src/version.rs"]
mod version;

use std::path::Path;
use std::process::Command;

/// Output of a Git command in this checkout, or `None` without Git or a repository.
fn git(arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").args(arguments).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn main() {
    for path in [
        "assets/scopework.ico",
        "BUILD_NUMBER",
        "Cargo.toml",
        "Cargo.lock",
        "build.rs",
        "src",
        "tests",
        "examples",
        "installer",
        "docs",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    // Worktrees keep their own HEAD and index while sharing references.
    for path in ["HEAD", "index", "refs", "packed-refs"] {
        if let Some(git_path) = git(&["rev-parse", "--git-path", path]) {
            println!("cargo:rerun-if-changed={git_path}");
        }
    }
    let cargo = std::env::var("CARGO_PKG_VERSION").expect("Cargo sets the package version");
    let product = version::product_version(&cargo).expect("Cargo.toml version");
    let build: u32 = std::fs::read_to_string("BUILD_NUMBER")
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0);
    let revision = git(&["rev-parse", "--verify", "HEAD"]).unwrap_or_else(|| "unknown".into());
    // The package script raises BUILD_NUMBER just before compiling and it is committed with
    // the package afterwards, so that one change does not make a package dirty.
    let dirty = git(&[
        "status",
        "--porcelain",
        "--untracked-files=normal",
        "--",
        ".",
        ":(exclude)BUILD_NUMBER",
    ])
    .is_some_and(|status| !status.is_empty());
    let display = format!("Scopework {product} (Build {build:06})");
    let out = std::env::var("OUT_DIR").expect("Cargo sets OUT_DIR");
    std::fs::write(
        Path::new(&out).join("build_identity.rs"),
        format!(
            "/// The product version, as people see it.\npub const PRODUCT: &str = {product:?};\n\
             /// The reserved package build number; 0 for a source build.\npub const BUILD: u32 = {build};\n\
             /// The Git revision compiled, or `unknown`.\npub const REVISION: &str = {revision:?};\n\
             /// Whether the checkout had uncommitted changes.\npub const DIRTY: bool = {dirty};\n\
             /// `Scopework <product> (Build <number>)`.\npub const DISPLAY: &str = {display:?};\n"
        ),
    )
    .expect("write the build identity");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/scopework.ico")
            .set("ProductName", "Scopework")
            .set("FileDescription", "Scopework Research Workbench")
            .set("CompanyName", "Scopework")
            .set("ProductVersion", &display)
            .compile()
            .expect("compile Windows icon and version resources");
    }
}
