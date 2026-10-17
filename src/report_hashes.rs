//! Report content hashes reused across status evaluations.
//!
//! Effective status compares each accepted report's SHA-256 with its accepted hash, so every
//! evaluation used to read and hash every completed report. A hash is reused while the file's
//! size and modification time are unchanged and that time is older than the moment the hash was
//! taken (git's racy-timestamp rule), so an edit in the same instant is never missed. Misses
//! are hashed in parallel on Solworker. Each thread keeps a Solcache table, so the GUI's
//! background refreshes reuse hashes; one-shot CLI calls load a persisted copy instead.
use super::*;
use solcache::{
    SCAccountingDomain, SCAllocationClass, SCByteLimit, SCCache, SCCleanupContext, SCLookup,
};
use std::{
    cell::RefCell,
    collections::HashMap,
    num::NonZeroUsize,
    time::{SystemTime, UNIX_EPOCH},
};

const PERSISTED: &str = "tools/workspaces/scopework-cache/report-hashes.json";
/// A modification time this close to (or after) the hashing moment is not trusted.
const RACY_NS: u64 = 2_000_000_000;
/// Evaluations smaller than this skip the persisted copy and the worker pool.
const BULK: usize = 64;
/// In-memory budget per thread before idle entries are collected.
const MEMORY_BYTES: u64 = 64 << 20;

#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Stamp {
    size: u64,
    mtime_ns: u64,
    /// When the hash was taken; the stamp is trusted only for files modified well before it.
    hashed_ns: u64,
    hash: Option<String>,
}
impl Stamp {
    fn reusable_for(&self, size: u64, mtime_ns: u64) -> bool {
        self.size == size
            && self.mtime_ns == mtime_ns
            && mtime_ns.saturating_add(RACY_NS) <= self.hashed_ns
    }
}

struct Memory {
    cleanup: &'static SCCleanupContext<Stamp>,
    domain: SCAccountingDomain,
    cache: SCCache<'static, PathBuf, Stamp>,
}
thread_local! {
    static MEMORY: RefCell<Option<Memory>> = const { RefCell::new(None) };
}
fn with_memory<R>(f: impl FnOnce(&mut Memory) -> R) -> R {
    MEMORY.with(|cell| {
        let mut slot = cell.borrow_mut();
        let memory = slot.get_or_insert_with(|| Memory {
            // One context per thread for the thread's lifetime; it is bound to this thread.
            cleanup: Box::leak(Box::new(SCCleanupContext::new())),
            domain: SCAccountingDomain::new(),
            cache: SCCache::new(),
        });
        f(memory)
    })
}
fn remembered(path: &Path) -> Option<Stamp> {
    with_memory(|m| match m.cache.lookup(&path.to_path_buf()) {
        SCLookup::Ready(view) => Some(view.get().clone()),
        _ => None,
    })
}
fn remember(path: PathBuf, stamp: Stamp) {
    with_memory(|m| {
        let cost = (path.as_os_str().len() + 128) as u64;
        if let Ok(backing) =
            m.cleanup
                .try_acquire(stamp, &m.domain, cost, SCAllocationClass::Resident)
        {
            let _ = m.cache.install(path, backing, cost);
        }
        if m.cache.policy_bytes() > MEMORY_BYTES {
            m.cache
                .maintain(SCByteLimit::Above(MEMORY_BYTES / 2), usize::MAX, false);
            m.cleanup.drain();
        }
    });
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
}
fn metadata(path: &Path) -> Option<(u64, u64)> {
    let meta = fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    Some((meta.len(), mtime))
}
fn hash_file(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = fs::read(path).ok()?;
    // Status hashes the report as text; a non-UTF-8 file is never a valid accepted report.
    std::str::from_utf8(&bytes).ok()?;
    Some(format!("{:x}", Sha256::digest(&bytes)))
}

/// SHA-256 of each report (relative to `dir`), or `None` when missing or not UTF-8.
pub(super) fn hashes(dir: &Path, reports: &[&str]) -> HashMap<String, Option<String>> {
    let bulk = reports.len() >= BULK;
    let persisted_path = dir.join(PERSISTED);
    let persisted: HashMap<String, Stamp> = if bulk {
        fs::read(&persisted_path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    } else {
        HashMap::new()
    };
    let mut out = HashMap::with_capacity(reports.len());
    let mut misses: Vec<(String, u64, u64, Option<String>)> = vec![];
    // Metadata calls dominate a warm evaluation on Windows, so bulk evaluations stat in parallel.
    let mut stats: Vec<(&str, Option<(u64, u64)>)> = reports.iter().map(|r| (*r, None)).collect();
    let runtime = bulk
        .then(|| {
            background::runtime(
                std::thread::available_parallelism()
                    .map_or(2, |n| n.get())
                    .clamp(2, 8),
            )
            .ok()
        })
        .flatten();
    let parallel_stats = runtime.as_ref().is_some_and(|runtime| {
        runtime
            .lane(solworker::SWExecutionClass::Low)
            .for_each_chunk(
                &mut stats,
                NonZeroUsize::new(64).unwrap(),
                |_, chunk: &mut [(&str, Option<(u64, u64)>)]| {
                    for entry in chunk {
                        entry.1 = metadata(&dir.join(entry.0));
                    }
                },
            )
            .ok()
            .and_then(|outcome| outcome.ok())
            .is_some()
    });
    if !parallel_stats {
        for entry in &mut stats {
            entry.1 = metadata(&dir.join(entry.0));
        }
    }
    for (rel, meta) in stats {
        let path = dir.join(rel);
        let Some((size, mtime)) = meta else {
            out.insert(rel.to_string(), None);
            continue;
        };
        let known = remembered(&path).or_else(|| persisted.get(rel).cloned());
        match known {
            Some(stamp) if stamp.reusable_for(size, mtime) => {
                remember(path, stamp.clone());
                out.insert(rel.to_string(), stamp.hash);
            }
            _ => misses.push((rel.to_string(), size, mtime, None)),
        }
    }
    if misses.is_empty() {
        return out;
    }
    let hashed_ns = now_ns();
    let parallel = misses.len() >= BULK
        && runtime
            .as_ref()
            .and_then(|runtime| {
                runtime
                    .lane(solworker::SWExecutionClass::Low)
                    .for_each_chunk(
                        &mut misses,
                        NonZeroUsize::new(32).unwrap(),
                        |_, chunk: &mut [(String, u64, u64, Option<String>)]| {
                            for miss in chunk {
                                miss.3 = hash_file(&dir.join(&miss.0));
                            }
                        },
                    )
                    .ok()
                    // A panicking chunk leaves hashes unset; fall back to hashing serially.
                    .and_then(|outcome| outcome.ok())
            })
            .is_some();
    if !parallel {
        for miss in &mut misses {
            miss.3 = hash_file(&dir.join(&miss.0));
        }
    }
    let mut updated = persisted;
    for (rel, size, mtime, hash) in misses {
        let stamp = Stamp {
            size,
            mtime_ns: mtime,
            hashed_ns,
            hash: hash.clone(),
        };
        remember(dir.join(&rel), stamp.clone());
        if bulk {
            updated.insert(rel.clone(), stamp);
        }
        out.insert(rel, hash);
    }
    if bulk {
        // Best effort: the copy only saves work, so a failed or concurrent write loses nothing.
        if let Some(parent) = persisted_path.parent()
            && fs::create_dir_all(parent).is_ok()
            && let Ok(bytes) = serde_json::to_vec(&updated)
        {
            let _ = atomic(&persisted_path, &bytes);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn workspace() -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-workspaces")
            .join(format!("hashes-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
    fn sha(text: &str) -> String {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(text.as_bytes()))
    }
    fn set_mtime(path: &Path, when: SystemTime) {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }

    #[test]
    fn bulk_hashes_match_contents_and_persist() {
        let dir = workspace();
        let names: Vec<String> = (0..BULK + 6).map(|i| format!("r{i}.md")).collect();
        for (i, name) in names.iter().enumerate() {
            fs::write(dir.join(name), format!("report {i}")).unwrap();
        }
        fs::write(dir.join("bad.md"), [0xffu8, 0xfe]).unwrap();
        let mut rels: Vec<&str> = names.iter().map(String::as_str).collect();
        rels.extend(["bad.md", "missing.md"]);
        let first = hashes(&dir, &rels);
        for (i, name) in names.iter().enumerate() {
            assert_eq!(first[name], Some(sha(&format!("report {i}"))));
        }
        assert_eq!(first["bad.md"], None, "non-UTF-8 reports never hash");
        assert_eq!(first["missing.md"], None);
        assert!(dir.join(PERSISTED).is_file(), "bulk evaluations persist");
        assert_eq!(hashes(&dir, &rels), first);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_change_inside_the_racy_window_is_always_rehashed() {
        let dir = workspace();
        let path = dir.join("r.md");
        fs::write(&path, "aaaa").unwrap();
        let mtime = SystemTime::now();
        set_mtime(&path, mtime);
        assert_eq!(hashes(&dir, &["r.md"])["r.md"], Some(sha("aaaa")));
        // Same size and the same modification time, but modified within the racy window.
        fs::write(&path, "bbbb").unwrap();
        set_mtime(&path, mtime);
        assert_eq!(hashes(&dir, &["r.md"])["r.md"], Some(sha("bbbb")));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_old_unchanged_file_reuses_its_stamp() {
        let dir = workspace();
        let path = dir.join("r.md");
        fs::write(&path, "aaaa").unwrap();
        let old = SystemTime::now() - Duration::from_secs(60);
        set_mtime(&path, old);
        assert_eq!(hashes(&dir, &["r.md"])["r.md"], Some(sha("aaaa")));
        // Identical size and time well before the hash was taken: the stamp is trusted,
        // which is the point of the cache (a content change always moves size or time).
        fs::write(&path, "bbbb").unwrap();
        set_mtime(&path, old);
        assert_eq!(hashes(&dir, &["r.md"])["r.md"], Some(sha("aaaa")));
        fs::write(&path, "bbbbb").unwrap();
        assert_eq!(hashes(&dir, &["r.md"])["r.md"], Some(sha("bbbbb")));
        fs::remove_dir_all(&dir).unwrap();
    }
}
