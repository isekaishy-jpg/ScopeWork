//! The executable's reported identity matches the version source and the committed build number.
use std::process::Command;

fn research(flag: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_research"))
        .arg(flag)
        .output()
        .unwrap();
    assert!(output.status.success(), "{flag} failed");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn version_and_build_info_agree_with_cargo_and_build_number() {
    let cargo = env!("CARGO_PKG_VERSION");
    let product = research_dashboard::version::product_version(cargo).unwrap();
    let build: u32 = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/BUILD_NUMBER"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let display = format!("Scopework {product} (Build {build:06})");
    assert_eq!(research("--version").trim(), display);
    assert_eq!(research("-V").trim(), display);
    let info: serde_json::Value = serde_json::from_str(&research("--build-info")).unwrap();
    assert_eq!(info["version"], product);
    assert_eq!(info["cargo_version"], cargo);
    assert_eq!(info["build"], build);
    assert_eq!(info["display"], display);
    assert!(info["revision"].as_str().is_some_and(|r| !r.is_empty()));
    assert!(info["dirty"].is_boolean());
}
