//! What this executable is: product version, package build number and source revision, as
//! captured by `build.rs` when it was compiled.
include!(concat!(env!("OUT_DIR"), "/build_identity.rs"));

/// The identity as `research --build-info` prints it.
pub fn json() -> serde_json::Value {
    serde_json::json!({
        "product": "Scopework",
        "version": PRODUCT,
        "cargo_version": env!("CARGO_PKG_VERSION"),
        "build": BUILD,
        "build_label": format!("{BUILD:06}"),
        "revision": REVISION,
        "dirty": DIRTY,
        "display": DISPLAY,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::{compare, key, product_version};
    use std::cmp::Ordering;

    #[test]
    fn cargo_versions_map_to_product_spellings() {
        assert_eq!(product_version("0.13.0-alpha").unwrap(), "0.13.0a");
        assert_eq!(product_version("0.13.0-beta").unwrap(), "0.13.0b");
        assert_eq!(product_version("0.13.0-rc").unwrap(), "0.13.0rc");
        assert_eq!(product_version("1.0.0").unwrap(), "1.0.0s");
        for bad in [
            "0.13.0-dev",
            "0.13",
            "0.13.0.1",
            "0.013.0",
            "x.1.2",
            "0.13.0-",
        ] {
            assert!(product_version(bad).is_err(), "{bad} accepted");
        }
        // This build's own identity agrees with Cargo.toml.
        assert_eq!(product_version(env!("CARGO_PKG_VERSION")).unwrap(), PRODUCT);
        assert_eq!(DISPLAY, format!("Scopework {PRODUCT} (Build {BUILD:06})"));
    }

    #[test]
    fn versions_order_by_number_then_stage_then_build() {
        // Stages: alpha < beta < rc < stable, in both spellings; older unsuffixed releases are stable.
        let order = [
            "0.8.0",
            "0.13.0a",
            "0.13.0-beta",
            "0.13.0rc",
            "0.13.0",
            "0.13.0s",
            "0.13.1-alpha",
        ];
        for pair in order.windows(2) {
            assert!(key(pair[0]) <= key(pair[1]), "{pair:?}");
        }
        assert!(key("0.13.0a") < key("0.13.0b"));
        assert_eq!(key("0.13.0"), key("0.13.0s"));
        assert_eq!(key("0.13.0-alpha"), key("0.13.0a"));
        assert_eq!(key("nonsense"), None);
        // Numbered packages order by build; a source build (0) falls back to the version.
        assert_eq!(
            compare("0.13.0-alpha", 7, "0.13.0-alpha", 5),
            Ordering::Greater
        );
        assert_eq!(
            compare("0.13.0-alpha", 0, "0.13.0-alpha", 5),
            Ordering::Equal
        );
        assert_eq!(compare("0.12.0", 0, "0.13.0-alpha", 0), Ordering::Less);
    }
}
