//! Product version spelling and ordering (the Solarity convention). Self-contained: `build.rs`
//! includes this file too, so it uses only the standard library.
//!
//! `Cargo.toml` is the version source. Its SemVer spelling maps to the product spelling shown
//! to people: `0.13.0-alpha` is `0.13.0a`, `-beta` is `b`, `-rc` is `rc`, and a plain
//! `0.13.0` is the stable `0.13.0s`.

/// The product spelling of a Cargo version.
pub fn product_version(cargo: &str) -> Result<String, &'static str> {
    let (base, suffix) = match cargo.split_once('-') {
        Some((base, "alpha")) => (base, "a"),
        Some((base, "beta")) => (base, "b"),
        Some((base, "rc")) => (base, "rc"),
        Some(_) => return Err("unsupported release stage: use -alpha, -beta, -rc or none"),
        None => (cargo, "s"),
    };
    numbers(base).ok_or("a version needs three numeric components without leading zeros")?;
    Ok(format!("{base}{suffix}"))
}

fn numbers(base: &str) -> Option<(u64, u64, u64)> {
    let parts: Vec<&str> = base.split('.').collect();
    if parts.len() != 3
        || parts.iter().any(|p| {
            p.is_empty()
                || !p.bytes().all(|b| b.is_ascii_digit())
                || (p.len() > 1 && p.starts_with('0'))
        })
    {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}

/// An orderable key for a Cargo or product spelling: the three numbers, then the stage
/// (alpha < beta < release candidate < stable). Versions without a stage, as published before
/// this convention, count as stable. `None` for anything else.
pub fn key(version: &str) -> Option<(u64, u64, u64, u8)> {
    let (base, stage) = if let Some((base, stage)) = version.split_once('-') {
        (
            base,
            match stage {
                "alpha" => 0,
                "beta" => 1,
                "rc" => 2,
                _ => return None,
            },
        )
    } else if let Some(base) = version.strip_suffix("rc") {
        (base, 2)
    } else if let Some(base) = version.strip_suffix('a') {
        (base, 0)
    } else if let Some(base) = version.strip_suffix('b') {
        (base, 1)
    } else if let Some(base) = version.strip_suffix('s') {
        (base, 3)
    } else {
        (version, 3)
    };
    let (major, minor, patch) = numbers(base)?;
    Some((major, minor, patch, stage))
}

/// How two builds order: by build number when both are numbered packages (numbers only ever
/// grow), otherwise by version. Unparsable versions order before parsable ones.
pub fn compare(a: &str, a_build: u32, b: &str, b_build: u32) -> std::cmp::Ordering {
    if a_build > 0 && b_build > 0 {
        return a_build.cmp(&b_build);
    }
    key(a).cmp(&key(b))
}
