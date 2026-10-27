//! Runtime drift detection (phase 2): changes to shared project files made outside the
//! operations that own them. Declared resource files (`tools/coordinator.toml`) belong to lease
//! holders. The catalog and sidecars belong to Scopework, whose every write raises the
//! catalog revision (or, for a layout migration, its layout version); a change to them with
//! neither is drift. Changes are found by polling stamps (size, modification time), then
//! hashes. Drift is recorded, never blocked or reverted.
use regex::Regex;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Instant, SystemTime};

pub(super) const RECEIPTS: &str = "tools/workspaces/catalog-transactions-v4";
const PENDING: &str = "tools/workspaces/catalog-transactions-v4/pending";
/// Sidecars that change only through catalog transactions.
const SIDECARS: [&str; 5] = [
    "review-flags.json",
    "claims.json",
    "review-queue.json",
    "quarantine.json",
    "proofs.json",
];
/// The annotation ledger, which catalog transactions repath in place when reports move.
const LEDGER: &str = "tools/profiles/annotations.json";

type Stamp = Option<(u64, SystemTime)>;
/// How close to a stamp's taking a modification time must be to distrust the stamp.
const RACY: std::time::Duration = std::time::Duration::from_secs(2);
fn stamp(path: &Path) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}
fn hash(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    std::fs::read(path)
        .ok()
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
}

/// One change found outside its owner.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Drift {
    pub resource: String,
    pub file: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Default)]
pub(super) struct Watch {
    /// When the previous check started; leases touched since then excuse a change.
    previous: Option<Instant>,
    manifest: Stamp,
    /// Declared files: path -> (resource, stamp, hash, when the stamp was taken).
    files: BTreeMap<String, (String, Stamp, Option<String>, SystemTime)>,
    /// The catalog and sidecars: path -> stamp.
    owned: BTreeMap<String, Stamp>,
    /// The catalog's revision and layout version last seen, and the stamp they were read at.
    revision: Option<u64>,
    layout: Option<u64>,
    catalog: Stamp,
    catalog_taken: Option<SystemTime>,
}

/// The catalog's revision and layout version, read from the first 64 KB of `project.json`
/// (catalogs list them among their first fields), falling back to a full parse.
fn catalog_head(path: &Path) -> (Option<u64>, Option<u64>) {
    use std::io::Read;
    let mut head = vec![0; 64 * 1024];
    let read = std::fs::File::open(path)
        .and_then(|mut f| f.read(&mut head))
        .unwrap_or(0);
    let head = String::from_utf8_lossy(&head[..read]);
    static REVISION: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static LAYOUT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let field = |pattern: &std::sync::OnceLock<Regex>, name: &str| {
        pattern
            .get_or_init(|| Regex::new(&format!(r#""{name}"\s*:\s*(\d+)"#)).unwrap())
            .captures(&head)?[1]
            .parse::<u64>()
            .ok()
    };
    match (
        field(&REVISION, "revision"),
        field(&LAYOUT, "layout_version"),
    ) {
        (Some(revision), layout) => (Some(revision), layout),
        _ => {
            let value = std::fs::read(path)
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
            let get = |name: &str| value.as_ref().and_then(|v| v.get(name)?.as_u64());
            (get("revision"), get("layout_version"))
        }
    }
}

impl Watch {
    /// One poll of `dir`. `excused(resource, since)` says whether a lease on the resource is
    /// held now or was granted or released since `since`. The first call only records.
    pub fn check(&mut self, dir: &Path, excused: &dyn Fn(&str, Instant) -> bool) -> Vec<Drift> {
        let started = Instant::now();
        let first = self.previous.is_none();
        let since = self.previous.unwrap_or(started);
        let mut found = vec![];

        // Declared files follow the manifest as it changes.
        let manifest_stamp = stamp(&dir.join(super::manifest::MANIFEST));
        if first || manifest_stamp != self.manifest {
            self.manifest = manifest_stamp;
            let declared = super::manifest::load(dir)
                .map(|m| m.watched())
                .unwrap_or_default();
            let mut files = BTreeMap::new();
            for (resource, file) in declared {
                let kept = self
                    .files
                    .remove(&file)
                    .map(|(_, s, h, t)| (s, h, t))
                    .unwrap_or_else(|| {
                        let path = dir.join(&file);
                        (stamp(&path), hash(&path), SystemTime::now())
                    });
                files.insert(file, (resource, kept.0, kept.1, kept.2));
            }
            self.files = files;
        }

        // Scopework's own catalog writes since the last check, and whether a transaction among
        // them repathed the ledger.
        // A pending journal, the rule recovery uses: an empty pending folder is reused by the
        // next commit and holds no transaction.
        let in_flight = dir.join(PENDING).join("journal.json").is_file();
        let catalog = dir.join("project.json");
        let catalog_stamp = stamp(&catalog);
        // The racy-timestamp rule again: a catalog rewritten within one clock tick keeps its stamp.
        let catalog_racy = catalog_stamp.is_some_and(|(_, modified)| {
            self.catalog_taken
                .is_none_or(|taken| modified + RACY > taken)
        });
        let (revision, layout) = if catalog_stamp == self.catalog && !first && !catalog_racy {
            (self.revision, self.layout)
        } else {
            catalog_head(&catalog)
        };
        let receipt = |r: u64| dir.join(RECEIPTS).join(format!("revision-{r}"));
        let mut ledger_repathed = false;
        if let (Some(old), Some(new)) = (self.revision, revision) {
            for r in old.saturating_add(1)..=new.min(old.saturating_add(10_000)) {
                if let Ok(journal) = std::fs::read(receipt(r).join("journal.json")) {
                    // Repathing (`ledger`) and re-pinning (`repin_ledger`) both rewrite it.
                    ledger_repathed |= journal.windows(13).any(|w| w == b"\"ledger\":true")
                        || journal.windows(19).any(|w| w == b"\"repin_ledger\":true");
                }
            }
        }
        let scopework_wrote = revision != self.revision || layout != self.layout;

        for owned in std::iter::once("project.json").chain(SIDECARS) {
            let path = dir.join(owned);
            let now = stamp(&path);
            let before = self.owned.insert(owned.to_string(), now);
            // A sidecar appearing is Scopework creating it empty just before its first
            // transaction (the transaction itself may land in the next poll).
            let created = before == Some(None);
            if first || in_flight || created || before.as_ref() == Some(&now) || scopework_wrote {
                continue;
            }
            found.push(Drift {
                resource: "catalog".into(),
                file: owned.to_string(),
                before: None,
                after: hash(&path),
            });
        }
        if !in_flight {
            self.revision = revision;
            self.layout = layout;
            self.catalog = catalog_stamp;
            self.catalog_taken = Some(SystemTime::now());
        }

        // A transaction in flight may already have rewritten the ledger, but its receipt does not
        // exist yet: judge declared files after it lands, with the lease window still open from
        // the last check that judged them.
        if in_flight && !first {
            return found;
        }
        for (file, (resource, old_stamp, old_hash, taken)) in self.files.iter_mut() {
            let path = dir.join(file.as_str());
            let now = stamp(&path);
            // A stamp is trusted only for a file modified well before it was taken (git's
            // racy-timestamp rule): two same-size writes within one timestamp tick look alike.
            let racy = now.is_some_and(|(_, modified)| modified + RACY > *taken);
            if now == *old_stamp && !racy {
                continue;
            }
            *old_stamp = now;
            *taken = SystemTime::now();
            let new_hash = hash(&path);
            if new_hash == *old_hash {
                continue;
            }
            let previous = std::mem::replace(old_hash, new_hash.clone());
            let owner_wrote = excused(resource, since) || (file == LEDGER && ledger_repathed);
            if !first && !owner_wrote {
                found.push(Drift {
                    resource: resource.clone(),
                    file: file.clone(),
                    before: previous,
                    after: new_hash,
                });
            }
        }
        self.previous = Some(started);
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Dir(PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn changes_without_their_owner_are_drift() {
        let dir = Dir(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-workspaces")
            .join(format!("drift-{}", uuid::Uuid::new_v4())));
        let d = &dir.0;
        std::fs::create_dir_all(d.join("tools/profiles")).unwrap();
        std::fs::write(
            d.join("tools/coordinator.toml"),
            "version = 1\n[resources.ledger]\nfiles = [\"tools/profiles/annotations.json\"]\n",
        )
        .unwrap();
        std::fs::write(d.join(LEDGER), "{}").unwrap();
        std::fs::write(
            d.join("project.json"),
            r#"{"layout_version":5,"revision":1}"#,
        )
        .unwrap();
        let mut watch = Watch::default();
        let nobody = |_: &str, _: Instant| false;
        assert!(
            watch.check(d, &nobody).is_empty(),
            "the first check only records"
        );

        // A ledger write without a lease is drift; with one it is not.
        std::fs::write(d.join(LEDGER), "{\"a\":1}").unwrap();
        let drift = watch.check(d, &nobody);
        assert_eq!(drift.len(), 1);
        assert_eq!(
            (drift[0].resource.as_str(), drift[0].file.as_str()),
            ("ledger", LEDGER)
        );
        assert!(
            drift[0].before.is_some()
                && drift[0].after.is_some()
                && drift[0].before != drift[0].after
        );
        std::fs::write(d.join(LEDGER), "{\"a\":2}").unwrap();
        assert!(
            watch
                .check(d, &|r: &str, _: Instant| r == "ledger")
                .is_empty()
        );
        // Rewriting the same bytes is no change.
        std::fs::write(d.join(LEDGER), "{\"a\":2}").unwrap();
        let found = watch.check(d, &nobody);
        assert!(found.is_empty(), "{found:?}");

        // A catalog change that raises the revision is Scopework's (a transaction, or a scope or
        // import write without a receipt); one that does not is drift.
        std::fs::create_dir_all(d.join(RECEIPTS).join("revision-2")).unwrap();
        std::fs::write(
            d.join(RECEIPTS).join("revision-2/journal.json"),
            r#"{"ledger":true}"#,
        )
        .unwrap();
        std::fs::write(
            d.join("project.json"),
            r#"{"layout_version":5,"revision":2}"#,
        )
        .unwrap();
        // That transaction repathed the ledger: its ledger change is not drift either.
        std::fs::write(d.join(LEDGER), "{\"a\":3}").unwrap();
        assert!(watch.check(d, &nobody).is_empty());
        std::fs::write(
            d.join("project.json"),
            r#"{"layout_version":5,"revision":3,"research_scope":"x"}"#,
        )
        .unwrap();
        assert!(watch.check(d, &nobody).is_empty());
        std::fs::write(
            d.join("project.json"),
            r#"{"layout_version":5,"revision":3,"research_scope":"yy"}"#,
        )
        .unwrap();
        let drift = watch.check(d, &nobody);
        assert_eq!(drift.len(), 1);
        assert_eq!(
            (drift[0].resource.as_str(), drift[0].file.as_str()),
            ("catalog", "project.json")
        );
        // A sidecar created empty ahead of its transaction is not drift; an edit without a
        // revision change is.
        std::fs::write(d.join("claims.json"), r#"{"schema":1,"claims":{}}"#).unwrap();
        assert!(watch.check(d, &nobody).is_empty());
        std::fs::write(d.join("claims.json"), r#"{"schema":1,"claims":{"2":{}}}"#).unwrap();
        let drift = watch.check(d, &nobody);
        assert_eq!(drift.len(), 1);
        assert_eq!(drift[0].file, "claims.json");
    }

    #[test]
    fn scopeworks_own_ledger_rewrites_are_not_drift() {
        let dir = Dir(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-workspaces")
            .join(format!("drift-{}", uuid::Uuid::new_v4())));
        let d = &dir.0;
        std::fs::create_dir_all(d.join("tools/profiles")).unwrap();
        std::fs::write(
            d.join("tools/coordinator.toml"),
            "version = 1\n[resources.ledger]\nfiles = [\"tools/profiles/annotations.json\"]\n",
        )
        .unwrap();
        std::fs::write(d.join(LEDGER), "{}").unwrap();
        std::fs::write(
            d.join("project.json"),
            r#"{"layout_version":5,"revision":1}"#,
        )
        .unwrap();
        let mut watch = Watch::default();
        let nobody = |_: &str, _: Instant| false;
        assert!(watch.check(d, &nobody).is_empty());
        // A transaction in flight has rewritten the ledger and the catalog, but its receipt is
        // still the pending folder: nothing is judged yet.
        std::fs::create_dir_all(d.join(PENDING)).unwrap();
        std::fs::write(d.join(PENDING).join("journal.json"), "{}").unwrap();
        std::fs::write(d.join(LEDGER), r#"{"report_sha256":"new"}"#).unwrap();
        std::fs::write(
            d.join("project.json"),
            r#"{"layout_version":5,"revision":2}"#,
        )
        .unwrap();
        assert!(watch.check(d, &nobody).is_empty());
        // It lands as a re-pin (a rename batch keeping completion), not a repath.
        std::fs::remove_dir_all(d.join(PENDING)).unwrap();
        std::fs::create_dir_all(d.join(RECEIPTS).join("revision-2")).unwrap();
        std::fs::write(
            d.join(RECEIPTS).join("revision-2/journal.json"),
            r#"{"ledger":false,"repin_ledger":true}"#,
        )
        .unwrap();
        let found = watch.check(d, &nobody);
        assert!(found.is_empty(), "{found:?}");
        // A later write by nobody is still drift.
        std::fs::write(d.join(LEDGER), r#"{"report_sha256":"tool"}"#).unwrap();
        assert_eq!(watch.check(d, &nobody).len(), 1);
    }
}
