//! Report content hashes reused across status evaluations.
//!
//! Effective status compares each accepted report's SHA-256 with its accepted hash, so every
//! evaluation used to read and hash every completed report. A hash is reused while the file's
//! size and modification time are unchanged and that time is older than the moment the hash was
//! taken (git's racy-timestamp rule), so an edit in the same instant is never missed. Misses
//! are hashed in parallel on Solworker. Long-lived threads that opt in with
//! `retain_on_this_thread` (the GUI's refresh thread, the coordinator's writers) keep a Solcache
//! table so their repeated evaluations reuse hashes. Other threads keep nothing in memory, so a
//! thread that ends leaves nothing behind; bulk evaluations load a persisted copy instead.
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
impl Drop for Memory {
    fn drop(&mut self) {
        // Dropping the table queues its entries on the context, which is never dropped itself.
        self.cache = SCCache::new();
        self.cleanup.drain();
    }
}
thread_local! {
    static RETAIN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static MEMORY: RefCell<Option<Memory>> = const { RefCell::new(None) };
}
/// Keeps report stamps in memory on the calling thread from now on. Only for threads that live
/// as long as their process: each one holds a table (within `MEMORY_BYTES`) and a cleanup context
/// that is never freed.
pub(crate) fn retain_on_this_thread() {
    RETAIN.with(|retain| retain.set(true));
}
/// Runs `f` on this thread's table, or returns `None` if the thread does not retain stamps.
fn with_memory<R>(f: impl FnOnce(&mut Memory) -> R) -> Option<R> {
    if !RETAIN.with(std::cell::Cell::get) {
        return None;
    }
    MEMORY.with(|cell| {
        let mut slot = cell.borrow_mut();
        let memory = slot.get_or_insert_with(|| Memory {
            // One context per retaining thread for the thread's lifetime; it is bound to it.
            cleanup: Box::leak(Box::new(SCCleanupContext::new())),
            domain: SCAccountingDomain::new(),
            cache: SCCache::new(),
        });
        Some(f(memory))
    })
}
fn remembered(path: &Path) -> Option<Stamp> {
    with_memory(|m| match m.cache.lookup(&path.to_path_buf()) {
        SCLookup::Ready(view) => Some(view.get().clone()),
        _ => None,
    })
    .flatten()
}
fn remember(path: PathBuf, stamp: Stamp) {
    let _ = with_memory(|m| {
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
        }
        // Replaced and evicted entries wait on the cleanup queue until drained; the table's
        // budget does not count them, so drain on every insert.
        m.cleanup.drain();
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

/// The shared runtime's Low workers for bulk passes, or `None` (serial) if it cannot start.
fn bulk_runtime() -> Option<&'static solworker::SWRuntime> {
    background::shared().ok()
}
/// Runs `work` over every item, in parallel chunks when a runtime is given and serially
/// otherwise or after a failed chunk (a panicking chunk leaves its items unprocessed).
fn for_each<T: Send>(
    items: &mut [T],
    runtime: Option<&solworker::SWRuntime>,
    chunk: usize,
    work: impl Fn(&mut T) + Sync,
) {
    let parallel = runtime.is_some_and(|runtime| {
        runtime
            .lane(solworker::SWExecutionClass::Low)
            .for_each_chunk(
                items,
                NonZeroUsize::new(chunk).unwrap(),
                |_, part: &mut [T]| part.iter_mut().for_each(&work),
            )
            .ok()
            .and_then(|outcome| outcome.ok())
            .is_some()
    });
    if !parallel {
        items.iter_mut().for_each(work);
    }
}
fn load_persisted<T: serde::de::DeserializeOwned>(path: &Path) -> HashMap<String, T> {
    fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
/// Best effort: the copy only saves work, so a failed or concurrent write loses nothing.
fn store_persisted<T: serde::Serialize>(path: &Path, value: &HashMap<String, T>) {
    if let Some(parent) = path.parent()
        && fs::create_dir_all(parent).is_ok()
        && let Ok(bytes) = serde_json::to_vec(value)
    {
        let _ = atomic(path, &bytes);
    }
}

/// SHA-256 of each report (relative to `dir`), or `None` when missing or not UTF-8.
pub(super) fn hashes(dir: &Path, reports: &[&str]) -> HashMap<String, Option<String>> {
    let bulk = reports.len() >= BULK;
    let persisted_path = dir.join(PERSISTED);
    let persisted: HashMap<String, Stamp> = if bulk {
        load_persisted(&persisted_path)
    } else {
        HashMap::new()
    };
    let mut out = HashMap::with_capacity(reports.len());
    let mut misses: Vec<(String, u64, u64, Option<String>)> = vec![];
    // Metadata calls dominate a warm evaluation on Windows, so bulk evaluations stat in parallel.
    let mut stats: Vec<(&str, Option<(u64, u64)>)> = reports.iter().map(|r| (*r, None)).collect();
    let runtime = bulk.then(bulk_runtime).flatten();
    for_each(&mut stats, runtime, 64, |entry| {
        entry.1 = metadata(&dir.join(entry.0))
    });
    for (rel, meta) in stats {
        let path = dir.join(rel);
        let Some((size, mtime)) = meta else {
            out.insert(rel.to_string(), None);
            continue;
        };
        let in_memory = remembered(&path);
        let from_memory = in_memory.is_some();
        let known = in_memory.or_else(|| persisted.get(rel).cloned());
        match known {
            Some(stamp) if stamp.reusable_for(size, mtime) => {
                // Only a stamp from the persisted copy needs installing; one from memory is there.
                if !from_memory {
                    remember(path, stamp.clone());
                }
                out.insert(rel.to_string(), stamp.hash);
            }
            _ => misses.push((rel.to_string(), size, mtime, None)),
        }
    }
    if misses.is_empty() {
        return out;
    }
    let hashed_ns = now_ns();
    let parallel = runtime.filter(|_| misses.len() >= BULK);
    for_each(&mut misses, parallel, 32, |miss| {
        miss.3 = hash_file(&dir.join(&miss.0))
    });
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
        store_persisted(&persisted_path, &updated);
    }
    out
}

// v2 also indexes values written with a `0x` prefix (citations such as `Name` (`0x00401000`));
// v3 indexes hexadecimal values of 8 to 16 digits, so VAs above 4 GiB are found too.
const REFERENCES: &str = "tools/workspaces/scopework-cache/report-references-v3.json";
/// The canonical folder names a report mentions, under the same reuse rule as hashes.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Mentions {
    size: u64,
    mtime_ns: u64,
    read_ns: u64,
    folders: Vec<String>,
}
/// Canonical folder names in `text`: 6-digit node IDs and hexadecimal values of 8 to 16 digits
/// (RVAs, and VAs including those above 4 GiB, also after a `0x` prefix), uppercase.
fn folder_names(text: &str) -> Vec<String> {
    static NAMES: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let names = NAMES.get_or_init(|| {
        regex::Regex::new(r"(?i)(?:^|[^0-9a-z_]|0x)([0-9a-f]{8,16}|[0-9]{6})\b").unwrap()
    });
    let mut found: Vec<String> = names
        .captures_iter(text)
        .map(|c| c[1].to_uppercase())
        .collect();
    found.sort_unstable();
    found.dedup();
    found
}
/// The reports among `reports` that mention any of `folders` (canonical folder names). Only
/// these can hold a reference into a moved or removed folder, so a structural change reads
/// them instead of every report. Unchanged reports are only statted; their names come from a
/// persisted index. Missing or unreadable reports are never returned.
pub(super) fn mentioning(
    dir: &Path,
    reports: &[&str],
    folders: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    let persisted_path = dir.join(REFERENCES);
    let persisted: HashMap<String, Mentions> = load_persisted(&persisted_path);
    let runtime = (reports.len() >= BULK).then(bulk_runtime).flatten();
    let mut stats: Vec<(&str, Option<(u64, u64)>)> = reports.iter().map(|r| (*r, None)).collect();
    for_each(&mut stats, runtime, 64, |entry| {
        entry.1 = metadata(&dir.join(entry.0))
    });
    let mut known: Vec<(String, Mentions)> = vec![];
    let mut misses: Vec<(String, u64, u64, Option<Vec<String>>)> = vec![];
    for (rel, meta) in stats {
        let Some((size, mtime)) = meta else { continue };
        match persisted.get(rel) {
            Some(m)
                if m.size == size
                    && m.mtime_ns == mtime
                    && mtime.saturating_add(RACY_NS) <= m.read_ns =>
            {
                known.push((rel.to_string(), m.clone()))
            }
            _ => misses.push((rel.to_string(), size, mtime, None)),
        }
    }
    let read_ns = now_ns();
    for_each(&mut misses, runtime, 32, |miss| {
        miss.3 = fs::read(dir.join(&miss.0))
            .ok()
            .map(|bytes| folder_names(&String::from_utf8_lossy(&bytes)))
    });
    let fresh: Vec<(String, Mentions)> = misses
        .into_iter()
        .filter_map(|(rel, size, mtime_ns, names)| {
            Some((
                rel,
                Mentions {
                    size,
                    mtime_ns,
                    read_ns,
                    folders: names?,
                },
            ))
        })
        .collect();
    let found = known
        .iter()
        .chain(&fresh)
        .filter(|(_, m)| m.folders.iter().any(|f| folders.contains(f)))
        .map(|(rel, _)| rel.clone())
        .collect();
    if !fresh.is_empty() {
        // Entries for reports outside this request stay: another call may ask for them.
        let mut all = persisted;
        all.extend(fresh);
        store_persisted(&persisted_path, &all);
        // Earlier formats of the index are rebuilt here; nothing reads them any more.
        for old in ["report-references.json", "report-references-v2.json"] {
            let _ = fs::remove_file(dir.join("tools/workspaces/scopework-cache").join(old));
        }
    }
    found
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
    fn writing_the_index_removes_its_earlier_formats() {
        let dir = workspace();
        let cache = dir.join("tools/workspaces/scopework-cache");
        fs::create_dir_all(&cache).unwrap();
        for old in ["report-references.json", "report-references-v2.json"] {
            fs::write(cache.join(old), "{}").unwrap();
        }
        fs::write(dir.join("a.md"), "see 00401000").unwrap();
        let wanted: std::collections::HashSet<String> = ["00401000".to_string()].into();
        assert_eq!(mentioning(&dir, &["a.md"], &wanted).len(), 1);
        assert!(cache.join("report-references-v3.json").is_file());
        assert!(!cache.join("report-references.json").exists());
        assert!(!cache.join("report-references-v2.json").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn citations_of_vas_above_four_gigabytes_are_indexed() {
        let names = folder_names("`Alloc` (`0x140001000`) beside ../000184/00401000/research.md");
        for name in ["140001000", "000184", "00401000"] {
            assert!(names.contains(&name.to_string()), "{name}: {names:?}");
        }
        assert!(folder_names("count 1234567 and 0x10").is_empty());
    }

    #[test]
    fn folder_mentions_come_from_the_index_until_a_report_changes() {
        let dir = workspace();
        let old = SystemTime::now() - Duration::from_secs(60);
        for (name, text) in [
            ("a.md", "see ../000184/003f4930/research.md"),
            ("b.md", "count 1234567 and 0x10, nothing canonical"),
        ] {
            fs::write(dir.join(name), text).unwrap();
            set_mtime(&dir.join(name), old);
        }
        let wanted: std::collections::HashSet<String> = ["003F4930".to_string()].into();
        let found = mentioning(&dir, &["a.md", "b.md", "gone.md"], &wanted);
        assert_eq!(found, ["a.md".to_string()].into());
        // Same size and old timestamp: the persisted index answers without reading the file.
        fs::write(dir.join("a.md"), "see ../000184/00000000/research.md").unwrap();
        set_mtime(&dir.join("a.md"), old);
        assert_eq!(mentioning(&dir, &["a.md"], &wanted), found);
        fs::write(dir.join("a.md"), "moved elsewhere entirely").unwrap();
        assert!(mentioning(&dir, &["a.md"], &wanted).is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_old_unchanged_file_reuses_its_stamp() {
        retain_on_this_thread();
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

    /// Writes `count` reports dated a minute ago, so their stamps are reusable.
    fn old_reports(dir: &Path, count: usize) -> Vec<String> {
        let old = SystemTime::now() - Duration::from_secs(60);
        (0..count)
            .map(|i| {
                let name = format!("r{i}.md");
                fs::write(dir.join(&name), format!("report {i}")).unwrap();
                set_mtime(&dir.join(&name), old);
                name
            })
            .collect()
    }

    #[test]
    fn a_thread_that_does_not_retain_keeps_nothing_in_memory() {
        let dir = workspace();
        let names = old_reports(&dir, 8);
        let dir_for_thread = dir.clone();
        let kept = std::thread::spawn(move || {
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            assert_eq!(hashes(&dir_for_thread, &refs).len(), refs.len());
            assert_eq!(hashes(&dir_for_thread, &refs).len(), refs.len());
            MEMORY.with(|cell| cell.borrow().is_some())
        })
        .join()
        .unwrap();
        assert!(!kept, "a short-lived thread must not create a stamp table");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn repeated_evaluations_leave_nothing_queued_for_cleanup() {
        let dir = workspace();
        let names = old_reports(&dir, 8);
        let dir_for_thread = dir.clone();
        let (cached, pending) = std::thread::spawn(move || {
            retain_on_this_thread();
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            for _ in 0..5 {
                hashes(&dir_for_thread, &refs);
            }
            // A changed report replaces its entry.
            fs::write(dir_for_thread.join(&names[0]), "changed").unwrap();
            hashes(&dir_for_thread, &refs);
            MEMORY
                .with(|cell| {
                    let slot = cell.borrow();
                    slot.as_ref().map(|m| (m.cache.len(), m.cleanup.snapshot()))
                })
                .unwrap()
        })
        .join()
        .unwrap();
        assert_eq!(cached, 8);
        assert_eq!((pending.pending_records, pending.in_flight_records), (0, 0));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn dropping_a_retained_table_frees_its_entries() {
        let dir = workspace();
        let names = old_reports(&dir, 8);
        let dir_for_thread = dir.clone();
        let (cached, pending) = std::thread::spawn(move || {
            retain_on_this_thread();
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            hashes(&dir_for_thread, &refs);
            let (cleanup, cached) = MEMORY
                .with(|cell| {
                    let slot = cell.borrow();
                    slot.as_ref().map(|m| (m.cleanup, m.cache.len()))
                })
                .unwrap();
            drop(MEMORY.with(|cell| cell.borrow_mut().take()));
            (cached, cleanup.snapshot())
        })
        .join()
        .unwrap();
        assert_eq!(cached, 8);
        assert_eq!((pending.pending_records, pending.in_flight_records), (0, 0));
        fs::remove_dir_all(&dir).unwrap();
    }
}
