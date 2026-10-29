//! The project store (docs/design-storage-backend.md): the catalog's whole history beside the
//! files, in checksummed frames, kept by following the change receipts every writer leaves.
//!
//! Until a project is promoted the files stay authoritative and nothing here takes part in a
//! commit: `sync` reads the current catalog under the project's shared lock, walks the receipts
//! back from it to the store's head, and appends what is new, so a store fault never changes the
//! files or fails a commit. A promoted project's commits append to the store themselves
//! ([`commit`]), and its history is here rather than in receipts
//! (docs/design-storage-promotion.md).
//!
//! Layout of `<project>/store/`:
//! - `store.json`: schema, project and codec.
//! - `segment-<n>.zlib`: the catalog (`project.json` bytes) where segment `n` starts, compressed.
//! - `segment-<n>.log`: frames following that checkpoint: a start entry, then one commit entry
//!   per revision. `store compact` compresses a closed segment's log to `segment-<n>.logz`
//!   (the same frames as one zlib stream).
//! - `texts-<n>.pack`: report and sidecar texts by SHA-256, compressed in groups.
//! - `ledger.log`, `ledger-head.zlib`: the annotation ledger followed ([`ledger`]).
//! - `.lock`: held by the one process writing the store.
//!
//! A frame is a 4-byte length, a 4-byte CRC-32 of the payload, then the payload (little-endian).
//! Readers take no lock: they read frames up to the last complete one.
use super::*;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::ops::Range;

#[path = "store_commit.rs"]
mod commit;
pub(crate) use commit::{append_commit, begin_commit, holds, receipts_after};
#[path = "store_ledger.rs"]
mod ledger;
pub(crate) use ledger::stamp as ledger_stamp;
pub use ledger::{annotations, conflicts};

pub(crate) const FOLDER: &str = "store";
const SCHEMA: u64 = 1;
/// A log longer than this is closed and a new segment, with a checkpoint, started.
const SEGMENT_BYTES: usize = 4 << 20;
/// `default`, or in debug builds the value of environment variable `name` when set, so tests
/// reach segment and compaction limits with a small history.
fn tunable(name: &str, default: usize) -> usize {
    #[cfg(debug_assertions)]
    if let Some(value) = std::env::var(name).ok().and_then(|v| v.parse().ok()) {
        return value;
    }
    let _ = name;
    default
}
/// [`SEGMENT_BYTES`], or `SCOPEWORK_STORE_SEGMENT_BYTES` in debug builds.
fn segment_bytes() -> usize {
    tunable("SCOPEWORK_STORE_SEGMENT_BYTES", SEGMENT_BYTES)
}
/// Texts are compressed together in groups of about this many bytes.
const PACK_BYTES: usize = 4 << 20;
/// A pack file longer than this is closed and the next one started.
const PACK_FILE_BYTES: u64 = 64 << 20;
/// Closed segment logs found whole, by size and modification time (see `Writer::check_closed`).
const CLOSED: &str = "closed.json";
/// A sync that leaves more small pack frames than this compacts the store.
const MAX_SMALL_FRAMES: usize = 64;
/// [`MAX_SMALL_FRAMES`], or `SCOPEWORK_STORE_MAX_SMALL_FRAMES` in debug builds.
fn max_small_frames() -> usize {
    tunable("SCOPEWORK_STORE_MAX_SMALL_FRAMES", MAX_SMALL_FRAMES)
}
const RECEIPT_FOLDERS: [&str; 2] = [
    "tools/workspaces/catalog-transactions-v4",
    "tools/workspaces/catalog-transactions",
];

fn frame(payload: &[u8]) -> Result<Vec<u8>> {
    let len = u32::try_from(payload.len()).map_err(|_| "Store frame too large")?;
    let mut out = Vec::with_capacity(payload.len() + 8);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&crc32fast::hash(payload).to_le_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}
/// How a sequence of frames ends.
#[derive(Clone, Copy, PartialEq, Debug)]
enum End {
    Clean,
    /// A frame still being written, or cut short by a crash, starts at this offset.
    Torn(usize),
    /// A complete frame with a wrong checksum starts at this offset.
    Damaged(usize),
}
/// The payload ranges of the complete frames in `bytes`, and how they end.
fn frames(bytes: &[u8]) -> (Vec<Range<usize>>, End) {
    let mut out = vec![];
    let mut at = 0;
    loop {
        if at == bytes.len() {
            return (out, End::Clean);
        }
        if at + 8 > bytes.len() {
            return (out, End::Torn(at));
        }
        let len = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let crc = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap());
        let body = at + 8;
        if body + len > bytes.len() {
            return (out, End::Torn(at));
        }
        if crc32fast::hash(&bytes[body..body + len]) != crc {
            return (out, End::Damaged(at));
        }
        out.push(body..body + len);
        at = body + len;
    }
}
fn deflate(bytes: &[u8]) -> Result<Vec<u8>> {
    deflate_at(bytes, flate2::Compression::default())
}
fn deflate_at(bytes: &[u8], level: flate2::Compression) -> Result<Vec<u8>> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), level);
    encoder.write_all(bytes).map_err(|e| e.to_string())?;
    encoder.finish().map_err(|e| e.to_string())
}
fn inflate(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut out = vec![];
    flate2::read::ZlibDecoder::new(bytes)
        .read_to_end(&mut out)
        .map_err(|e| format!("Store data does not decompress: {e}"))?;
    Ok(out)
}
fn segment_files(store: &Path, seq: u64) -> (PathBuf, PathBuf) {
    (
        store.join(format!("segment-{seq:06}.log")),
        store.join(format!("segment-{seq:06}.zlib")),
    )
}
/// Files in `store` named `<prefix><number><suffix>`, by number.
fn numbered(store: &Path, prefix: &str, suffix: &str) -> Vec<(u64, PathBuf)> {
    let mut out: Vec<(u64, PathBuf)> = fs::read_dir(store)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let n = name
                .strip_prefix(prefix)?
                .strip_suffix(suffix)?
                .parse()
                .ok()?;
            Some((n, entry.path()))
        })
        .collect();
    out.sort_by_key(|(n, _)| *n);
    out
}

/// One segment: a checkpoint and the log that follows it.
struct Segment {
    seq: u64,
    log: PathBuf,
    checkpoint: PathBuf,
    bytes: Vec<u8>,
    frames: Vec<Range<usize>>,
    end: End,
    start: Value,
}
impl Segment {
    fn entry(&self, i: usize) -> Result<Value> {
        serde_json::from_slice(&self.bytes[self.frames[i].clone()]).map_err(|e| e.to_string())
    }
    /// The revision and catalog fingerprint this segment ends at.
    fn head(&self) -> Result<(u64, String)> {
        let last = self.entry(self.frames.len() - 1)?;
        let (revision, sha) = if last["type"] == "commit" {
            (&last["revision"], &last["after_sha256"])
        } else {
            (&last["revision"], &last["catalog_sha256"])
        };
        Ok((
            revision.as_u64().ok_or("Store entry without revision")?,
            sha.as_str()
                .ok_or("Store entry without fingerprint")?
                .into(),
        ))
    }
}
/// Every segment's log by number: the plain log, else its compressed copy.
fn segment_logs(store: &Path) -> Vec<(u64, PathBuf)> {
    let mut logs = numbered(store, "segment-", ".log");
    let plain: HashSet<u64> = logs.iter().map(|(n, _)| *n).collect();
    logs.extend(
        numbered(store, "segment-", ".logz")
            .into_iter()
            .filter(|(n, _)| !plain.contains(n)),
    );
    logs.sort_by_key(|(n, _)| *n);
    logs
}
fn compressed(path: &Path) -> bool {
    path.extension().is_some_and(|x| x == "logz")
}
/// A log's frames, decompressed when it is a closed log compressed by `store compact`.
fn read_log(path: &Path) -> Result<Vec<u8>> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    if compressed(path) {
        inflate(&bytes)
    } else {
        Ok(bytes)
    }
}
/// Whether `log` is what an interrupted segment creation leaves: no complete frame at all.
/// Anything more (entries without a start, a missing checkpoint, a damaged frame) is damage.
fn unfinished(log: &Path) -> Result<bool> {
    let bytes = read_log(log)?;
    let (frames, end) = frames(&bytes);
    Ok(frames.is_empty() && !matches!(end, End::Damaged(_)))
}
/// Segment `seq` read from `log`, or none when it has no complete start entry or no checkpoint
/// (a segment whose creation was interrupted).
fn load_segment(store: &Path, seq: u64, log: PathBuf) -> Result<Option<Segment>> {
    let (_, checkpoint) = segment_files(store, seq);
    let bytes = read_log(&log)?;
    let (frames, end) = frames(&bytes);
    let start = frames
        .first()
        .and_then(|r| serde_json::from_slice::<Value>(&bytes[r.clone()]).ok())
        .filter(|v| v["type"] == "start");
    Ok(match start {
        Some(start) if checkpoint.is_file() => Some(Segment {
            seq,
            log,
            checkpoint,
            bytes,
            frames,
            end,
            start,
        }),
        _ => None,
    })
}
/// The store's segments in order, and the logs that are not part of it (see [`load_segment`]).
fn segments(store: &Path) -> Result<(Vec<Segment>, Vec<PathBuf>)> {
    let mut out = vec![];
    let mut invalid = vec![];
    for (seq, log) in segment_logs(store) {
        match load_segment(store, seq, log.clone())? {
            Some(segment) => out.push(segment),
            None => invalid.push(log),
        }
    }
    Ok((out, invalid))
}
fn checkpoint_catalog(segment: &Segment) -> Result<Vec<u8>> {
    inflate(&fs::read(&segment.checkpoint).map_err(|e| e.to_string())?)
}

/// Where each stored text is: pack file, frame payload range, and its range in the frame's
/// uncompressed texts.
struct TextIndex {
    at: HashMap<String, (PathBuf, Range<usize>, Range<usize>)>,
    problems: Vec<String>,
    /// Pack frames holding fewer than [`PACK_BYTES`] of texts: each sync writes one, and
    /// compaction merges them.
    small_frames: usize,
}
fn text_index(store: &Path) -> Result<TextIndex> {
    let mut at = HashMap::new();
    let mut problems = vec![];
    let mut small_frames = 0;
    for (_, pack) in numbered(store, "texts-", ".pack") {
        let bytes = fs::read(&pack).map_err(|e| e.to_string())?;
        let (frames, end) = frames(&bytes);
        if let End::Damaged(offset) = end {
            problems.push(format!(
                "{}: damaged frame at byte {offset}",
                pack.display()
            ));
        }
        for range in frames {
            let Some((index, _)) = pack_parts(&bytes[range.clone()]) else {
                problems.push(format!("{}: unreadable pack index", pack.display()));
                continue;
            };
            if index.iter().map(|(_, t)| t.len()).sum::<usize>() < PACK_BYTES {
                small_frames += 1;
            }
            for (sha, texts) in index {
                at.insert(sha, (pack.clone(), range.clone(), texts));
            }
        }
    }
    Ok(TextIndex {
        at,
        problems,
        small_frames,
    })
}
/// Each text of a pack frame: its SHA-256 and range in the frame's uncompressed texts.
type PackIndex = Vec<(String, Range<usize>)>;
/// A pack frame's index and the offset of its compressed texts.
fn pack_parts(payload: &[u8]) -> Option<(PackIndex, usize)> {
    let len = u32::from_le_bytes(payload.get(..4)?.try_into().ok()?) as usize;
    let index: Vec<Value> = serde_json::from_slice(payload.get(4..4 + len)?).ok()?;
    let index = index
        .iter()
        .map(|row| {
            let offset = row["offset"].as_u64()? as usize;
            let length = row["length"].as_u64()? as usize;
            Some((row["sha256"].as_str()?.to_string(), offset..offset + length))
        })
        .collect::<Option<Vec<_>>>()?;
    Some((index, 4 + len))
}

/// The one process writing a store: its lock and the texts waiting to be packed.
struct Writer {
    store: PathBuf,
    _lock: fs::File,
    known: HashSet<String>,
    pending: Vec<(String, String)>,
    pending_bytes: usize,
    added: usize,
    /// [`TextIndex::small_frames`], kept current as frames are written.
    small_frames: usize,
}
impl Writer {
    fn open(store: &Path) -> Result<Self> {
        fs::create_dir_all(store).map_err(|e| e.to_string())?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(store.join(".lock"))
            .map_err(|e| e.to_string())?;
        lock.try_lock().map_err(|_| {
            "store_busy: another process is writing this project's store. Retry shortly."
                .to_string()
        })?;
        let mut known = HashSet::new();
        let mut small_frames = 0;
        for (_, pack) in numbered(store, "texts-", ".pack") {
            small_frames += pack_texts(&pack, &mut known)?;
        }
        Ok(Self {
            store: store.into(),
            _lock: lock,
            known,
            pending: vec![],
            pending_bytes: 0,
            added: 0,
            small_frames,
        })
    }
    /// The segment commits append to, after the repairs an append needs: temporaries removed,
    /// an unfinished new segment removed, and frames cut short at the end of the active log, the
    /// last pack and the ledger log truncated. Unlike [`repair`](Self::repair) it reads only the
    /// active log, so a commit's cost does not grow with the history.
    fn active(&self) -> Result<Segment> {
        self.active_or_none()?.ok_or_else(|| {
            "store_mismatch: the store has no segment; run research store verify".into()
        })
    }
    /// [`active`](Self::active), or none for a store with no segment yet (at `init`).
    fn active_or_none(&self) -> Result<Option<Segment>> {
        for entry in fs::read_dir(&self.store)
            .map_err(|e| e.to_string())?
            .flatten()
        {
            if entry.file_name().to_string_lossy().ends_with(".pending") {
                let _ = fs::remove_file(entry.path());
            }
        }
        let tails = numbered(&self.store, "texts-", ".pack")
            .pop()
            .map(|(_, p)| p)
            .into_iter()
            .chain(Some(self.store.join(ledger::LOG)).filter(|p| p.is_file()));
        for path in tails {
            truncate_torn(&path)?;
        }
        let mut logs = segment_logs(&self.store);
        while let Some((seq, log)) = logs.pop() {
            if !compressed(&log) {
                truncate_torn(&log)?;
            }
            match load_segment(&self.store, seq, log.clone())? {
                Some(segment) => {
                    self.check_closed(seq)?;
                    return Ok(Some(segment));
                }
                // A new segment whose creation was interrupted, as `repair` removes it. A log
                // with entries but no start or checkpoint is damage, never removed.
                None if unfinished(&log)? => {
                    let _ = fs::remove_file(&log);
                    let _ = fs::remove_file(segment_files(&self.store, seq).1);
                }
                None => {
                    return Err(format!(
                        "store_damaged: {} has no valid start or checkpoint; run research store verify",
                        log.display()
                    ));
                }
            }
        }
        Ok(None)
    }
    /// Refuses a store whose closed segments (those before `active`) are damaged, as `repair`
    /// does, without reading them all each time: a log found whole is remembered by its size
    /// and modification time in `closed.json`, and read again only when they change. Damage
    /// that leaves both unchanged is found by `store verify`.
    fn check_closed(&self, active: u64) -> Result<()> {
        let path = self.store.join(CLOSED);
        let mut known: HashMap<String, (u64, u128)> = fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let mut changed = false;
        for (seq, log) in segment_logs(&self.store) {
            if seq >= active {
                continue;
            }
            let meta = fs::metadata(&log).map_err(|e| e.to_string())?;
            let stamp = (
                meta.len(),
                meta.modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos()),
            );
            let name = log.file_name().unwrap().to_string_lossy().into_owned();
            if known.get(&name) == Some(&stamp) {
                continue;
            }
            let whole = load_segment(&self.store, seq, log.clone())?;
            if whole.is_none_or(|s| s.end != End::Clean) {
                return Err(format!(
                    "store_damaged: {} is not whole; run research store verify",
                    log.display()
                ));
            }
            known.insert(name, stamp);
            changed = true;
        }
        if changed {
            known.retain(|name, _| self.store.join(name).is_file());
            // Best effort: losing it only means reading the closed logs again.
            if let Ok(bytes) = serde_json::to_vec(&known) {
                let _ = atomic(&path, &bytes);
            }
        }
        Ok(())
    }
    /// [`open`](Self::open), waiting up to `wait` for another process writing the store.
    fn open_waiting(store: &Path, wait: std::time::Duration) -> Result<Self> {
        let deadline = std::time::Instant::now() + wait;
        loop {
            match Self::open(store) {
                Err(e) if e.starts_with("store_busy") && std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                result => return result,
            }
        }
    }
    /// Removes what an interrupted writer left: temporaries, segments whose creation did not
    /// finish, and a frame cut short at the end of the last log or pack. Damage anywhere else
    /// is refused: `store verify` reports it, and nothing is rewritten.
    fn repair(&self) -> Result<()> {
        for entry in fs::read_dir(&self.store)
            .map_err(|e| e.to_string())?
            .flatten()
        {
            if entry.file_name().to_string_lossy().ends_with(".pending") {
                let _ = fs::remove_file(entry.path());
            }
        }
        // A compressed copy left beside its log by an interrupted compaction.
        for (n, copy) in numbered(&self.store, "segment-", ".logz") {
            if segment_files(&self.store, n).0.is_file() {
                let _ = fs::remove_file(copy);
            }
        }
        let (segments, invalid) = segments(&self.store)?;
        let last = segments.last().map_or(0, |s| s.seq);
        for log in invalid {
            let seq = segment_logs(&self.store)
                .into_iter()
                .find(|(_, path)| *path == log)
                .map(|(n, _)| n);
            if seq.is_some_and(|n| n < last) || !unfinished(&log)? {
                return Err(format!(
                    "store_damaged: {} has no valid start or checkpoint; run research store verify",
                    log.display()
                ));
            }
            let _ = fs::remove_file(&log);
            if let Some(n) = seq {
                let _ = fs::remove_file(segment_files(&self.store, n).1);
            }
        }
        // A checkpoint whose log was never created.
        for (n, checkpoint) in numbered(&self.store, "segment-", ".zlib") {
            if n > last {
                let _ = fs::remove_file(checkpoint);
            }
        }
        let ledger_log = Some(self.store.join(ledger::LOG)).filter(|p| p.is_file());
        let tails = segments
            .last()
            .map(|s| s.log.clone())
            .filter(|log| !compressed(log))
            .into_iter()
            .chain(
                numbered(&self.store, "texts-", ".pack")
                    .pop()
                    .map(|(_, p)| p),
            )
            .chain(ledger_log);
        for path in tails {
            truncate_torn(&path)?;
        }
        // Earlier segments must be whole: only the last log is ever appended to.
        for segment in segments.iter().rev().skip(1) {
            if segment.end != End::Clean {
                return Err(format!(
                    "store_damaged: {} does not end cleanly; run research store verify",
                    segment.log.display()
                ));
            }
        }
        Ok(())
    }
    /// The SHA-256 of `text`, which is packed unless the store already holds it.
    fn text(&mut self, text: &str) -> Result<String> {
        let sha = hash(text);
        if self.known.insert(sha.clone()) {
            self.pending_bytes += text.len();
            self.pending.push((sha.clone(), text.to_string()));
            if self.pending_bytes >= PACK_BYTES {
                self.flush_texts()?;
            }
        }
        Ok(sha)
    }
    /// Appends the waiting texts as one compressed frame, flushed to disk.
    fn flush_texts(&mut self) -> Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let pending = std::mem::take(&mut self.pending);
        self.added += pending.len();
        if self.pending_bytes < PACK_BYTES {
            self.small_frames += 1;
        }
        self.pending_bytes = 0;
        let payload = pack_frame(
            pending
                .iter()
                .map(|(sha, text)| (sha.as_str(), text.as_bytes())),
        )?;
        let packs = numbered(&self.store, "texts-", ".pack");
        let path = match packs.last() {
            Some((n, path)) => {
                if fs::metadata(path).map_err(|e| e.to_string())?.len() < PACK_FILE_BYTES {
                    path.clone()
                } else {
                    self.store.join(format!("texts-{:06}.pack", n + 1))
                }
            }
            None => self.store.join("texts-000001.pack"),
        };
        append(&path, &frame(&payload)?)
    }
}
/// Adds the texts pack file `path` holds to `known` and answers how many of its frames are
/// small (see [`TextIndex::small_frames`]). Only each frame's header and index are read; the
/// compressed texts are skipped, so opening the writer does not read the texts themselves.
/// Checksums are `store verify`'s to check; a frame cut short at the end is ignored, as readers
/// ignore it.
fn pack_texts(path: &Path, known: &mut HashSet<String>) -> Result<usize> {
    use std::io::{Seek, SeekFrom};
    let mut file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let end = file.metadata().map_err(|e| e.to_string())?.len();
    let mut small = 0;
    let mut at = 0u64;
    let mut header = [0u8; 12];
    while at + 12 <= end {
        file.seek(SeekFrom::Start(at))
            .and_then(|_| file.read_exact(&mut header))
            .map_err(|e| e.to_string())?;
        let payload = u64::from(u32::from_le_bytes(header[..4].try_into().unwrap()));
        let index_len = u32::from_le_bytes(header[8..].try_into().unwrap()) as u64;
        if at + 8 + payload > end || index_len + 4 > payload {
            break;
        }
        let mut index = vec![0; index_len as usize];
        file.read_exact(&mut index).map_err(|e| e.to_string())?;
        let rows: Vec<Value> = serde_json::from_slice(&index)
            .map_err(|e| format!("{}: unreadable pack index: {e}", path.display()))?;
        let bytes: u64 = rows.iter().filter_map(|r| r["length"].as_u64()).sum();
        if bytes < PACK_BYTES as u64 {
            small += 1;
        }
        known.extend(
            rows.iter()
                .filter_map(|r| r["sha256"].as_str().map(str::to_string)),
        );
        at += 8 + payload;
    }
    Ok(small)
}
/// A pack frame's payload: an index of `texts` and their bytes, compressed together.
fn pack_frame<'a>(texts: impl Iterator<Item = (&'a str, &'a [u8])>) -> Result<Vec<u8>> {
    let mut index = vec![];
    let mut all = vec![];
    for (sha, text) in texts {
        index.push(json!({"sha256": sha, "offset": all.len(), "length": text.len()}));
        all.extend_from_slice(text);
    }
    let index = serde_json::to_vec(&index).map_err(|e| e.to_string())?;
    let mut payload = (index.len() as u32).to_le_bytes().to_vec();
    payload.extend_from_slice(&index);
    payload.extend_from_slice(&deflate(&all)?);
    Ok(payload)
}
/// Cuts a frame left incomplete at the end of `path` by an interrupted append. A complete frame
/// with a wrong checksum is damage, which is refused: `store verify` reports it.
fn truncate_torn(path: &Path) -> Result<()> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    match frames(&bytes).1 {
        End::Clean => Ok(()),
        End::Torn(at) => {
            let file = fs::OpenOptions::new()
                .write(true)
                .open(path)
                .map_err(|e| e.to_string())?;
            file.set_len(at as u64).map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())
        }
        End::Damaged(at) => Err(format!(
            "store_damaged: {} has a damaged frame at byte {at}; run research store verify",
            path.display()
        )),
    }
}
fn append(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.sync_data())
        .map_err(|e| format!("Cannot write {}: {e}", path.display()))
}

/// Undoes (`forward` false) or applies one receipt delta's record changes to `records` in place.
/// Every record taken out must be at its stated position with its stated content.
fn replay<T: PartialEq + serde::de::DeserializeOwned>(
    records: &mut Vec<T>,
    changes: &Value,
    forward: bool,
) -> Result<()> {
    let (out, into) = if forward {
        ("before", "after")
    } else {
        ("after", "before")
    };
    let rows = |side: &str| -> Result<Vec<(usize, T)>> {
        let mut rows = changes[side]
            .as_array()
            .ok_or("Invalid receipt delta")?
            .iter()
            .map(|row| {
                Ok((
                    row["index"].as_u64().ok_or("Invalid delta index")? as usize,
                    serde_json::from_value(row["record"].clone()).map_err(|e| e.to_string())?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        rows.sort_by_key(|(i, _)| *i);
        Ok(rows)
    };
    let remove = rows(out)?;
    let insert = rows(into)?;
    for (i, record) in &remove {
        if records.get(*i) != Some(record) {
            return Err("Receipt delta does not match the catalog".into());
        }
    }
    // Changed records that kept their positions (the common case) are replaced in place.
    if remove.len() == insert.len() && remove.iter().zip(&insert).all(|(a, b)| a.0 == b.0) {
        for (i, record) in insert {
            records[i] = record;
        }
        return Ok(());
    }
    for (i, _) in remove.iter().rev() {
        records.remove(*i);
    }
    for (i, record) in insert {
        if i > records.len() {
            return Err("Receipt delta index outside the catalog".into());
        }
        records.insert(i, record);
    }
    Ok(())
}
/// `project` with its own fields replaced by `header` (a receipt's `before` or `after`).
fn with_header(header: &Value, project: &mut Project) -> Result<()> {
    let mut header = header.clone();
    let object = header.as_object_mut().ok_or("Invalid receipt header")?;
    object.insert("nodes".into(), Value::Array(vec![]));
    object.insert("dependencies".into(), Value::Array(vec![]));
    let mut next: Project = serde_json::from_value(header).map_err(|e| e.to_string())?;
    next.nodes = std::mem::take(&mut project.nodes);
    next.dependencies = std::mem::take(&mut project.dependencies);
    *project = next;
    Ok(())
}
/// Applies (`forward`) or undoes one commit entry or receipt on `project`.
fn step(project: &mut Project, delta: &Value, forward: bool) -> Result<()> {
    let changes = if delta["type"] == "commit" {
        delta
    } else {
        &delta["changes"]
    };
    replay(&mut project.nodes, &changes["nodes"], forward)?;
    replay(&mut project.dependencies, &changes["dependencies"], forward)?;
    with_header(&delta[if forward { "after" } else { "before" }], project)
}

/// A receipt's facts as a commit entry, with texts replaced by their hashes (packed by `w`).
fn commit_entry(receipt: &Value, revision: u64, at: Option<u64>, w: &mut Writer) -> Result<Value> {
    let mut text = |v: &Value| -> Result<Value> {
        Ok(match v.as_str() {
            Some(t) => w.text(t)?.into(),
            None => Value::Null,
        })
    };
    let mut reports = vec![];
    for edit in receipt["edits"].as_array().into_iter().flatten() {
        reports.push(json!({
            "id": edit["id"],
            "old_path": edit["old"],
            "new_path": edit["new"],
            "before_sha256": text(&edit["before"])?,
            "after_sha256": text(&edit["after"])?,
        }));
    }
    let mut sidecars = vec![];
    for s in receipt["sidecars"].as_array().into_iter().flatten() {
        sidecars.push(if s.get("compacted").is_some() {
            // Compacted receipts keep only the hashes of these texts.
            json!({"path": s["path"], "before_sha256": s["before_sha256"], "after_sha256": s["after_sha256"], "stored": false})
        } else {
            json!({"path": s["path"], "before_sha256": text(&s["before"])?, "after_sha256": text(&s["after"])?, "stored": true})
        });
    }
    Ok(json!({
        "type": "commit",
        "revision": revision,
        "at_unix": receipt["at_unix"].as_u64().or(at),
        "directed_by": receipt["directed_by"],
        "before_sha256": receipt["before_hash"],
        "after_sha256": receipt["after_hash"],
        "before": receipt["before"],
        "after": receipt["after"],
        "nodes": receipt["changes"]["nodes"],
        "dependencies": receipt["changes"]["dependencies"],
        "reports": reports,
        "moves": receipt["moves"],
        "dependency_edits": receipt["dependencies"],
        "ledger": receipt["ledger"],
        "repin_ledger": receipt["repin_ledger"],
        "sidecars": sidecars,
    }))
}

/// The receipt of `revision`: its journal and file time. Looked up by name, so a sync that
/// steps back over a few revisions touches only those receipts, not every receipt there is.
fn receipt(dir: &Path, revision: u64) -> Option<(PathBuf, Option<u64>)> {
    RECEIPT_FOLDERS.iter().find_map(|folder| {
        let journal = dir
            .join(folder)
            .join(format!("revision-{revision}"))
            .join("journal.json");
        let meta = fs::metadata(&journal).ok().filter(|m| m.is_file())?;
        let at = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        Some((journal, at))
    })
}
fn project_dir(root: &Path, id: &str) -> Result<PathBuf> {
    if !valid_id(id) {
        return Err("Invalid project identity".into());
    }
    let dir = root.join(id);
    layout::inside(root, &dir)?;
    if !dir.join("project.json").is_file() {
        return Err(format!("Unknown project: {id}"));
    }
    Ok(dir)
}

/// A catalog reached by walking receipts back, kept as a checkpoint candidate: how many entries
/// back it lies, its revision, fingerprint and compressed bytes.
struct Mark {
    back: usize,
    revision: u64,
    sha: String,
    compressed: Vec<u8>,
}

/// Brings the store of project `id` to the catalog's current revision and records the ledger's
/// change; with `create`, creates it first (the manual migration step) from as much receipt
/// history as can be verified.
pub fn sync(root: &Path, id: &str, create: bool) -> Result<Value> {
    sync_with(root, id, create, true)
}
/// [`sync`] without the ledger, which the coordinator follows beside its writer
/// ([`follow_ledger`]) because comparing a large ledger takes seconds.
pub(crate) fn sync_catalog(root: &Path, id: &str) -> Result<Value> {
    sync_with(root, id, false, false)
}
/// Records the ledger's change since the store last followed it.
pub(crate) fn follow_ledger(root: &Path, id: &str) -> Result<Value> {
    let (dir, store) = open_store(root, id)?;
    let mut w = Writer::open(&store)?;
    w.active_or_none()?;
    let (revision, _) = current(&dir)?;
    ledger::follow_inner(&mut w, &dir, &store, revision, None)
}
/// Runs `change`, which writes the ledger, holding the store's writer lock, then records the
/// ledger's change naming `by`. The coordinator's follower only tries that lock, so it cannot
/// record the change first and lose its author.
pub(crate) fn ledger_change<T>(
    root: &Path,
    id: &str,
    by: &str,
    change: impl FnOnce() -> Result<T>,
) -> Result<(T, Value)> {
    let (dir, store) = open_store(root, id)?;
    let mut w = Writer::open_waiting(&store, std::time::Duration::from_secs(30))?;
    w.active_or_none()?;
    let out = change()?;
    let (revision, _) = current(&dir)?;
    // The change is written; a problem recording it is answered, and the next sync records it.
    let recorded = ledger::follow_inner(&mut w, &dir, &store, revision, Some(by))
        .unwrap_or_else(|e| json!({"error": e}));
    Ok((out, recorded))
}
/// The ledger file hash the store recorded last, if it has recorded the ledger at all.
pub(crate) fn ledger_recorded(dir: &Path) -> Result<Option<String>> {
    ledger::last_file_sha(&dir.join(FOLDER))
}
fn sync_with(root: &Path, id: &str, create: bool, with_ledger: bool) -> Result<Value> {
    let started = std::time::Instant::now();
    let dir = project_dir(root, id)?;
    let store = dir.join(FOLDER);
    let marker = store.join("store.json");
    if create && marker.is_file() {
        return Err("This project already has a store; use research store sync.".into());
    }
    if !create && !marker.is_file() {
        return Err(format!(
            "no_store: project {id} has no store. Creating one is a manual step: research store init {id}"
        ));
    }
    // The current catalog, read under the project's shared lock so no commit is half written.
    // Every store writer takes the project's lock before the store's, and the store's lock is
    // only ever tried while holding it, so a sync never waits for a commit that waits for it.
    let locks = acquire_locks(root, id, false, false)?;
    let pending = RECEIPT_FOLDERS
        .iter()
        .any(|f| dir.join(f).join("pending/journal.json").is_file());
    if pending {
        return Err("recovery_required: a catalog transaction is pending. Open the project to recover it, then sync.".into());
    }
    let bytes = fs::read(dir.join("project.json")).map_err(|e| e.to_string())?;
    let mut w = Writer::open(&store)?;
    drop(locks);
    // Only the active segment is read, so a sync's cost does not grow with the history;
    // `store verify` checks the closed segments.
    let last = w.active_or_none()?;
    let head = last.as_ref().map(Segment::head).transpose()?;
    let current_sha = sha256_hex(&bytes);
    let mut state: Project = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let current_revision = state.revision;
    if head
        .as_ref()
        .is_some_and(|(r, sha)| *r == current_revision && *sha == current_sha)
    {
        if create {
            atomic(&marker, &store_marker(id)?)?;
        }
        let ledger = with_ledger.then(|| ledger::follow(&mut w, &dir, &store, current_revision));
        return Ok(
            json!({"backend":"store","project":id,"revision":current_revision,"commits_added":0,"up_to_date":true,"ledger":ledger}),
        );
    }

    if promoted(&state) {
        return Err(
            "store_mismatch: a promoted project's commits write its store, and its head is not the current catalog; run research store verify"
                .into(),
        );
    }
    // Walk back from the current catalog, newest receipt first.

    let mut entries: Vec<Vec<u8>> = vec![];
    let mut marks = vec![Mark {
        back: 0,
        revision: current_revision,
        sha: current_sha.clone(),
        compressed: deflate(&bytes)?,
    }];
    drop(bytes);
    let mut revision = current_revision;
    let mut expect = current_sha.clone();
    let mut since_mark = 0usize;
    let mut connected = false;
    let stopped: String = loop {
        if let Some((head_revision, head_sha)) = &head {
            if revision == *head_revision && expect == *head_sha {
                connected = true;
                break String::new();
            }
            if revision <= *head_revision {
                break format!(
                    "the receipts reach revision {revision} with another catalog than the store's head"
                );
            }
        }
        let Some((path, at)) = receipt(&dir, revision) else {
            break format!("no receipt for revision {revision}");
        };
        let receipt: Value = match fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string()))
        {
            Ok(v) => v,
            Err(e) => break format!("receipt for revision {revision} unreadable: {e}"),
        };
        let delta = (receipt["kind"] == "completed-catalog-delta"
            || receipt["kind"] == "pending-catalog-delta")
            && receipt["schema"] == 1;
        let previous = receipt["before"]["revision"].as_u64();
        if !delta || receipt["after"]["revision"] != revision || previous.is_none() {
            break format!("receipt for revision {revision} is not a catalog delta");
        }
        if receipt["after_hash"] != expect.as_str() {
            break format!("receipt for revision {revision} does not match the catalog after it");
        }
        if let Err(e) = step(&mut state, &receipt, false) {
            break format!("receipt for revision {revision}: {e}");
        }
        let entry = commit_entry(&receipt, revision, at, &mut w)?;
        let entry = serde_json::to_vec(&entry).map_err(|e| e.to_string())?;
        since_mark += entry.len();
        entries.push(entry);
        expect = receipt["before_hash"]
            .as_str()
            .ok_or("Receipt without fingerprint")?
            .into();
        revision = previous.unwrap_or_default();
        if since_mark >= segment_bytes() {
            let rebuilt = catalog_bytes(&state)?;
            if sha256_hex(&rebuilt) != expect {
                break format!(
                    "the catalog rebuilt at revision {revision} does not match its recorded fingerprint"
                );
            }
            marks.push(Mark {
                back: entries.len(),
                revision,
                sha: expect.clone(),
                compressed: deflate(&rebuilt)?,
            });
            since_mark = 0;
        }
    };
    if !connected && marks.last().is_none_or(|m| m.back != entries.len()) {
        // Where the walk stopped becomes the oldest checkpoint, once it verifies.
        let rebuilt = catalog_bytes(&state)?;
        if sha256_hex(&rebuilt) == expect {
            marks.push(Mark {
                back: entries.len(),
                revision,
                sha: expect.clone(),
                compressed: deflate(&rebuilt)?,
            });
        }
    }
    drop(state);
    // Entries older than the oldest checkpoint cannot be anchored without the store's head.
    let oldest = if connected {
        entries.len()
    } else {
        let back = marks.last().map_or(0, |m| m.back);
        entries.truncate(back);
        back
    };
    // Texts first, so every entry written below finds its texts.
    w.flush_texts()?;

    let mut next_seq = last.as_ref().map_or(1, |s| s.seq + 1);
    let mut started_segments = vec![];
    let mut write_segment = |mark: &Mark, follows: Value| -> Result<PathBuf> {
        let (log, checkpoint) = segment_files(&store, next_seq);
        next_seq += 1;
        atomic(&checkpoint, &mark.compressed)?;
        let mut start = json!({"type":"start","revision":mark.revision,"catalog_sha256":mark.sha,"at_unix":unix_now()});
        if let Value::Object(extra) = follows {
            start.as_object_mut().unwrap().extend(extra);
        }
        append(
            &log,
            &frame(&serde_json::to_vec(&start).map_err(|e| e.to_string())?)?,
        )?;
        started_segments.push(mark.revision);
        Ok(log)
    };
    let write_entries = |log: &Path, range: Range<usize>, entries: &[Vec<u8>]| -> Result<()> {
        let mut out = vec![];
        // `entries` is newest first.
        for entry in entries[range].iter().rev() {
            out.extend_from_slice(&frame(entry)?);
        }
        if out.is_empty() {
            Ok(())
        } else {
            append(log, &out)
        }
    };
    // Checkpoints from oldest to newest. The current catalog's starts a segment only when
    // nothing older could be verified and the store does not connect.
    let mut used: Vec<&Mark> = marks
        .iter()
        .filter(|m| m.back > 0 && m.back <= oldest)
        .collect();
    used.sort_by_key(|m| std::cmp::Reverse(m.back));
    used.dedup_by_key(|m| m.back);
    if used.is_empty() && !connected {
        used.push(&marks[0]);
    }
    let mut log = if connected {
        last.as_ref().map(|s| s.log.clone())
    } else {
        None
    };
    let mut newer_end = oldest;
    for (i, mark) in used.iter().enumerate() {
        if let Some(log) = &log {
            write_entries(log, mark.back..newer_end, &entries)?;
        }
        let follows = if i == 0 && !connected {
            if create {
                json!({"follows":"init","history_from":mark.revision,"stopped":stopped})
            } else {
                json!({"follows":"gap","gap":{"from":head.as_ref().map(|h| h.0),"to":mark.revision,"reason":stopped}})
            }
        } else {
            json!({"follows":"commit"})
        };
        log = Some(write_segment(mark, follows)?);
        newer_end = mark.back;
    }
    let log = log.ok_or("Store has no segment to append to")?;
    write_entries(&log, 0..newer_end, &entries)?;
    // A long log is closed with a checkpoint of the current catalog.
    let long = fs::metadata(&log).map_err(|e| e.to_string())?.len() as usize >= segment_bytes();
    if long && newer_end > 0 {
        write_segment(&marks[0], json!({"follows":"commit"}))?;
    }
    let ledger = with_ledger.then(|| ledger::follow(&mut w, &dir, &store, current_revision));
    // Frequent syncs each write a small pack frame; past a limit they are merged.
    let compacted = (w.small_frames > max_small_frames())
        .then(|| compact_store(&store))
        .transpose()?;
    if create {
        atomic(&marker, &store_marker(id)?)?;
    }
    Ok(json!({
        "backend": "store",
        "project": id,
        "ledger": ledger,
        "from": head.as_ref().map(|h| h.0),
        "revision": current_revision,
        "commits_added": entries.len(),
        "segments_started": started_segments,
        "gap": (!connected && !create).then(|| json!({"from": head.as_ref().map(|h| h.0), "reason": stopped})),
        "history_from": create.then(|| used.first().map(|m| m.revision)).flatten(),
        "stopped": (!connected).then_some(stopped),
        "texts_added": w.added,
        "compacted": compacted,
        "took_ms": started.elapsed().as_millis() as u64,
    }))
}

/// Merges the store's packs into as few as possible and compresses closed segments' logs; the
/// answers of every read stay the same.
pub fn compact(root: &Path, id: &str) -> Result<Value> {
    let (_, store) = open_store(root, id)?;
    let w = Writer::open(&store)?;
    w.repair()?;
    let started = std::time::Instant::now();
    let mut out = compact_store(&store)?;
    out["backend"] = "store".into();
    out["project"] = id.into();
    out["took_ms"] = (started.elapsed().as_millis() as u64).into();
    Ok(out)
}
/// [`compact`] for a store whose writer lock the caller holds.
fn compact_store(store: &Path) -> Result<Value> {
    // Closed logs: every segment's but the last, which is still appended to. The compressed
    // copy is published before the log is removed; `repair` removes a copy left beside its log.
    let (segments, _) = segments(store)?;
    let mut logs = 0;
    for segment in segments.iter().rev().skip(1) {
        if !compressed(&segment.log) {
            atomic(
                &segment.log.with_extension("logz"),
                &deflate(&segment.bytes)?,
            )?;
            fs::remove_file(&segment.log).map_err(|e| e.to_string())?;
            logs += 1;
        }
    }
    // Packs: every distinct text once, in groups, into new pack files published by renaming
    // them into place before the old ones are removed.
    let old = numbered(store, "texts-", ".pack");
    let small_before = text_index(store)?.small_frames;
    let mut next = old.last().map_or(1, |(n, _)| n + 1);
    let mut written: Vec<PathBuf> = vec![];
    if small_before > 1 {
        let mut seen = HashSet::new();
        let mut group: Vec<(String, Vec<u8>)> = vec![];
        let mut group_bytes = 0;
        let mut emit = |group: &mut Vec<(String, Vec<u8>)>,
                        written: &mut Vec<PathBuf>|
         -> Result<()> {
            if group.is_empty() {
                return Ok(());
            }
            let payload = pack_frame(group.iter().map(|(sha, t)| (sha.as_str(), t.as_slice())))?;
            group.clear();
            let full = written.last().is_none_or(|p| {
                fs::metadata(p).map(|m| m.len()).unwrap_or_default() >= PACK_FILE_BYTES
            });
            if full {
                written.push(store.join(format!("texts-{next:06}.pack.pending")));
                next += 1;
            }
            append(written.last().unwrap(), &frame(&payload)?)
        };
        for (_, pack) in &old {
            let bytes = fs::read(pack).map_err(|e| e.to_string())?;
            for range in frames(&bytes).0 {
                let Some((index, offset)) = pack_parts(&bytes[range.clone()]) else {
                    return Err(format!(
                        "{}: unreadable pack index; run research store verify",
                        pack.display()
                    ));
                };
                let all = inflate(&bytes[range.start + offset..range.end])?;
                for (sha, text) in index {
                    let text = all.get(text).ok_or("Store text outside its pack")?;
                    if seen.insert(sha.clone()) {
                        group_bytes += text.len();
                        group.push((sha, text.to_vec()));
                        if group_bytes >= PACK_BYTES {
                            emit(&mut group, &mut written)?;
                            group_bytes = 0;
                        }
                    }
                }
            }
        }
        emit(&mut group, &mut written)?;
        for pending in &written {
            fs::rename(pending, pending.with_extension("")).map_err(|e| e.to_string())?;
        }
        for (_, pack) in &old {
            fs::remove_file(pack).map_err(|e| e.to_string())?;
        }
    }
    Ok(json!({
        "logs_compressed": logs,
        "small_frames_before": small_before,
        "small_frames_after": text_index(store)?.small_frames,
        "pack_files": numbered(store, "texts-", ".pack").len(),
    }))
}

fn store_marker(id: &str) -> Result<Vec<u8>> {
    serde_json::to_vec_pretty(&json!({"schema": SCHEMA, "project": id, "codec": "zlib"}))
        .map_err(|e| e.to_string())
}
fn open_store(root: &Path, id: &str) -> Result<(PathBuf, PathBuf)> {
    let dir = project_dir(root, id)?;
    let store = dir.join(FOLDER);
    if !store.join("store.json").is_file() {
        return Err(format!(
            "no_store: project {id} has no store. Creating one is a manual step: research store init {id}"
        ));
    }
    Ok((dir, store))
}
/// The current catalog's revision and fingerprint (read without a lock: a reader of files).
fn current(dir: &Path) -> Result<(u64, String)> {
    #[derive(Deserialize)]
    struct Revision {
        revision: u64,
    }
    let bytes = fs::read(dir.join("project.json")).map_err(|e| e.to_string())?;
    let revision: Revision = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    Ok((revision.revision, sha256_hex(&bytes)))
}
fn size(paths: impl IntoIterator<Item = PathBuf>) -> u64 {
    paths
        .into_iter()
        .filter_map(|p| fs::metadata(p).ok())
        .map(|m| m.len())
        .sum()
}

/// Head, lag, segments, gaps and sizes of a project's store.
pub fn status(root: &Path, id: &str) -> Result<Value> {
    let (dir, store) = open_store(root, id)?;
    let (segments, invalid) = segments(&store)?;
    let head = segments.last().map(Segment::head).transpose()?;
    let (revision, sha) = current(&dir)?;
    let gaps: Vec<Value> = segments
        .iter()
        .filter(|s| s.start["follows"] == "gap")
        .map(|s| s.start["gap"].clone())
        .collect();
    let commits: usize = segments.iter().map(|s| s.frames.len() - 1).sum();
    let packs: Vec<PathBuf> = numbered(&store, "texts-", ".pack")
        .into_iter()
        .map(|(_, p)| p)
        .collect();
    let texts = text_index(&store)?;
    Ok(json!({
        "backend": "store",
        "project": id,
        "head": head.as_ref().map(|(r, s)| json!({"revision": r, "catalog_sha256": s})),
        "catalog_revision": revision,
        "current": head.as_ref().is_some_and(|(r, s)| *r == revision && *s == sha),
        "behind": head.as_ref().map(|(r, _)| revision.saturating_sub(*r)),
        "history_from": segments.first().map(|s| &s.start["revision"]),
        "segments": segments.len(),
        "commits": commits,
        "gaps": gaps,
        "texts": texts.at.len(),
        "log_bytes": size(segments.iter().map(|s| s.log.clone())),
        "checkpoint_bytes": size(segments.iter().map(|s| s.checkpoint.clone())),
        "pack_bytes": size(packs),
        "small_pack_frames": texts.small_frames,

        "unfinished": invalid.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        "problems": texts.problems,
        "ledger": ledger::status(&store).unwrap_or_else(|e| json!({"error": e})),
        "ledger_bytes": size([store.join(ledger::LOG)]),
    }))
}

/// Checks every frame, checkpoint, commit chain and text, and the head against the catalog.
pub fn verify(root: &Path, id: &str) -> Result<Value> {
    let (dir, store) = open_store(root, id)?;
    let (segments, invalid) = segments(&store)?;
    let texts = text_index(&store)?;
    let mut problems = texts.problems.clone();
    let mut notes = vec![];
    for log in &invalid {
        problems.push(format!(
            "{}: no valid start entry or checkpoint",
            log.display()
        ));
    }
    let mut commits = 0usize;
    let mut referenced = HashSet::new();
    let mut previous_end: Option<String> = None;
    for (i, segment) in segments.iter().enumerate() {
        let name = segment.log.display();
        match segment.end {
            End::Clean => {}
            End::Torn(at) if i + 1 == segments.len() => {
                notes.push(format!("{name}: the frame at byte {at} is incomplete (a write in progress, or left by an interrupted one and removed by the next sync)"));
            }
            End::Torn(at) | End::Damaged(at) => {
                problems.push(format!("{name}: damaged frame at byte {at}"));
            }
        }
        let catalog = match checkpoint_catalog(segment) {
            Ok(c) => c,
            Err(e) => {
                problems.push(format!("{}: {e}", segment.checkpoint.display()));
                previous_end = None;
                continue;
            }
        };
        let sha = sha256_hex(&catalog);
        if segment.start["catalog_sha256"] != sha.as_str() {
            problems.push(format!(
                "{}: checkpoint does not match its fingerprint",
                segment.checkpoint.display()
            ));
        }
        if segment.start["follows"] == "commit" && previous_end.as_deref() != Some(sha.as_str()) {
            problems.push(format!("{name}: does not continue the previous segment"));
        }
        let mut state: Project = match serde_json::from_slice(&catalog) {
            Ok(p) => p,
            Err(e) => {
                problems.push(format!("{}: {e}", segment.checkpoint.display()));
                previous_end = None;
                continue;
            }
        };
        let mut chain = sha;
        let mut broken = false;
        for f in 1..segment.frames.len() {
            let entry = match segment.entry(f) {
                Ok(e) => e,
                Err(e) => {
                    problems.push(format!("{name}: entry {f}: {e}"));
                    broken = true;
                    break;
                }
            };
            commits += 1;
            for side in ["before_sha256", "after_sha256"] {
                for row in entry["reports"].as_array().into_iter().flatten() {
                    if let Some(sha) = row[side].as_str() {
                        referenced.insert(sha.to_string());
                    }
                }
                for row in entry["sidecars"].as_array().into_iter().flatten() {
                    if row["stored"] == true
                        && let Some(sha) = row[side].as_str()
                    {
                        referenced.insert(sha.to_string());
                    }
                }
            }
            if entry["before_sha256"] != chain.as_str() {
                problems.push(format!(
                    "{name}: revision {} does not follow the one before it",
                    entry["revision"]
                ));
                broken = true;
                break;
            }
            if let Err(e) = step(&mut state, &entry, true) {
                problems.push(format!("{name}: revision {}: {e}", entry["revision"]));
                broken = true;
                break;
            }
            chain = entry["after_sha256"]
                .as_str()
                .unwrap_or_default()
                .to_string();
        }
        if broken {
            previous_end = None;
            continue;
        }
        // One fingerprint per segment: the catalog the replay ends with.
        let end = sha256_hex(&catalog_bytes(&state)?);
        if end != chain {
            problems.push(format!(
                "{name}: replay ends with another catalog than recorded"
            ));
        }
        previous_end = Some(chain);
    }
    ledger::verify(&store, &mut problems, &mut referenced);
    // Every pack frame is decompressed once and each of its texts checked against its hash.
    let mut present = HashSet::new();
    for (_, pack) in numbered(&store, "texts-", ".pack") {
        let bytes = fs::read(&pack).map_err(|e| e.to_string())?;
        for range in frames(&bytes).0 {
            let Some((index, offset)) = pack_parts(&bytes[range.clone()]) else {
                continue;
            };
            let all = match inflate(&bytes[range.start + offset..range.end]) {
                Ok(all) => all,
                Err(e) => {
                    problems.push(format!("{}: {e}", pack.display()));
                    continue;
                }
            };
            for (sha, text) in index {
                if all.get(text).is_some_and(|t| sha256_hex(t) == sha) {
                    present.insert(sha);
                } else {
                    problems.push(format!("text {sha} does not match its hash"));
                }
            }
        }
    }
    for sha in &referenced {
        if !present.contains(sha) {
            problems.push(format!("text {sha} is missing"));
        }
    }
    let checked = present.len();
    let (revision, sha) = current(&dir)?;
    let head = segments.last().map(Segment::head).transpose()?;
    Ok(json!({
        "backend": "store",
        "project": id,
        "ok": problems.is_empty(),
        "problems": problems,
        "notes": notes,
        "segments": segments.len(),
        "commits": commits,
        "texts_checked": checked,
        "head": head.as_ref().map(|(r, s)| json!({"revision": r, "catalog_sha256": s})),
        "current": head.as_ref().is_some_and(|(r, s)| *r == revision && *s == sha),
        "catalog_revision": revision,
    }))
}

impl TextIndex {
    fn read(&self, sha: &str) -> Result<Option<String>> {
        let Some((pack, frame, range)) = self.at.get(sha) else {
            return Ok(None);
        };
        // Only this text's frame is read and decompressed.
        let mut file = match fs::File::open(pack) {
            Ok(file) => file,
            // Readers take no lock: a compaction may have replaced the packs since this index
            // was built. Index the store again and look once more.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let store = pack.parent().ok_or("Store pack without folder")?;
                let fresh = text_index(store)?;
                return match fresh.at.get(sha) {
                    Some((again, _, _)) if again != pack => fresh.read(sha),
                    _ => Err(format!("{}: {e}", pack.display())),
                };
            }
            Err(e) => return Err(e.to_string()),
        };
        let mut payload = vec![0; frame.len()];
        std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(frame.start as u64))
            .and_then(|_| file.read_exact(&mut payload))
            .map_err(|e| e.to_string())?;
        let (_, offset) = pack_parts(&payload).ok_or("Unreadable store pack")?;
        let texts = inflate(&payload[offset..])?;
        let text = texts
            .get(range.clone())
            .ok_or("Store text outside its pack")?;
        String::from_utf8(text.to_vec())
            .map(Some)
            .map_err(|e| e.to_string())
    }
}

/// Whether an entry's payload names record `id` as a record, a report, a move or a dependent
/// (`"id":N` or `"dependent":N`), so only such entries are parsed.
fn mentions(payload: &[u8], id: u64) -> bool {
    let id = id.to_string();
    ["\"id\":", "\"dependent\":"].iter().any(|key| {
        let key = key.as_bytes();
        payload.windows(key.len()).enumerate().any(|(at, w)| {
            if w != key {
                return false;
            }
            let digits = &payload[at + key.len()..];
            digits.starts_with(id.as_bytes())
                && !digits.get(id.len()).is_some_and(|b| b.is_ascii_digit())
        })
    })
}
/// The records on one side of a delta whose `field` is `id`.
fn rows<'a>(delta: &'a Value, side: &str, field: &str, id: u64) -> Vec<&'a Value> {
    delta[side]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| row["record"][field] == id)
        .map(|row| &row["record"])
        .collect()
}
/// The report change for record `id` in a commit entry.
fn report_row(entry: &Value, id: u64) -> Option<&Value> {
    entry["reports"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|r| r["id"] == id)
}

/// The revisions that changed record `id` or its report, newest first.
pub fn history(root: &Path, project: &str, id: u64) -> Result<Value> {
    let (_, store) = open_store(root, project)?;
    let (segments, _) = segments(&store)?;
    let mut out = vec![];
    for segment in &segments {
        for range in segment.frames.iter().skip(1) {
            let payload = &segment.bytes[range.clone()];
            if !mentions(payload, id) {
                continue;
            }
            let entry: Value = serde_json::from_slice(payload).map_err(|e| e.to_string())?;
            let before = rows(&entry["nodes"], "before", "id", id);
            let after = rows(&entry["nodes"], "after", "id", id);
            let mut row = json!({"revision": entry["revision"], "at_unix": entry["at_unix"], "directed_by": entry["directed_by"]});
            match (before.first(), after.first()) {
                (None, Some(_)) => row["created"] = true.into(),
                (Some(_), None) => row["removed"] = true.into(),
                (Some(Value::Object(a)), Some(Value::Object(b))) => {
                    let fields: Vec<&String> = a
                        .keys()
                        .chain(b.keys().filter(|k| !a.contains_key(*k)))
                        .filter(|k| a.get(*k) != b.get(*k))
                        .collect();
                    if !fields.is_empty() {
                        row["fields"] = json!(fields);
                    }
                    for key in ["name", "status", "parent", "length"] {
                        if a.get(key) != b.get(key) {
                            row[key] = json!({"before": a.get(key), "after": b.get(key)});
                        }
                    }
                }
                _ => {}
            }
            if let Some(report) = report_row(&entry, id) {
                row["report"] = report.clone();
            }
            if let Some(m) = entry["moves"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|m| m["id"] == id)
            {
                row["moved"] = json!({"from": m["old"], "to": m["new"]});
            }
            let prerequisites = |side| -> Vec<Value> {
                rows(&entry["dependencies"], side, "dependent", id)
                    .iter()
                    .map(|r| r["prerequisite"].clone())
                    .collect()
            };
            let (gone, added) = (prerequisites("before"), prerequisites("after"));
            if gone != added {
                row["prerequisites"] = json!({
                    "removed": gone.iter().filter(|p| !added.contains(p)).collect::<Vec<_>>(),
                    "added": added.iter().filter(|p| !gone.contains(p)).collect::<Vec<_>>(),
                });
            }
            // Revision, time and direction alone: the record was only mentioned.
            if row.as_object().is_some_and(|o| o.len() > 3) {
                out.push(row);
            }
        }
    }
    out.reverse();
    Ok(json!({
        "backend": "store",
        "project": project,
        "id": id,
        "history_from": segments.first().map(|s| &s.start["revision"]),
        "gaps": segments
            .iter()
            .filter(|s| s.start["follows"] == "gap")
            .map(|s| &s.start["gap"])
            .collect::<Vec<_>>(),
        "revisions": out,
    }))
}

/// The segment holding `revision`: the last one starting at or before it, once the store's
/// history covers that revision.
fn segment_at(segments: &[Segment], revision: u64) -> Result<usize> {
    let first = segments.first().ok_or("The store holds no history yet")?;
    let from = first.start["revision"].as_u64().unwrap_or_default();
    if revision < from {
        return Err(format!(
            "Revision {revision} is older than the store's history, which starts at {from}."
        ));
    }
    let (head, _) = segments[segments.len() - 1].head()?;
    if revision > head {
        return Err(format!(
            "Revision {revision} is newer than the store's head ({head}); run research store sync."
        ));
    }
    Ok(segments
        .iter()
        .rposition(|s| s.start["revision"].as_u64().is_some_and(|r| r <= revision))
        .unwrap_or(0))
}
/// The catalog at `revision`, rebuilt from its segment's checkpoint and checked against the
/// fingerprint recorded for it.
fn catalog_at(segment: &Segment, revision: u64) -> Result<(Project, String)> {
    let checkpoint = checkpoint_catalog(segment)?;
    let mut state: Project = serde_json::from_slice(&checkpoint).map_err(|e| e.to_string())?;
    let mut expect = segment.start["catalog_sha256"]
        .as_str()
        .ok_or("Store segment without fingerprint")?
        .to_string();
    for f in 1..segment.frames.len() {
        let entry = segment.entry(f)?;
        if entry["revision"].as_u64().is_none_or(|r| r > revision) {
            break;
        }
        if entry["before_sha256"] != expect.as_str() {
            return Err(format!(
                "Store revision {} does not follow the one before it; run research store verify",
                entry["revision"]
            ));
        }
        step(&mut state, &entry, true)?;
        expect = entry["after_sha256"].as_str().unwrap_or_default().into();
    }
    let sha = sha256_hex(&catalog_bytes(&state)?);
    if sha != expect {
        return Err(
            "The rebuilt catalog does not match its recorded fingerprint; run research store verify"
                .into(),
        );
    }
    if state.revision != revision {
        return Err(format!(
            "The store has no catalog at revision {revision}: it goes from {} to the next recorded revision.",
            state.revision
        ));
    }
    Ok((state, sha))
}

/// The whole catalog at `revision`, written to `out` (a new file) when given.
pub fn catalog(root: &Path, project: &str, revision: u64, out: Option<&Path>) -> Result<Value> {
    let (_, store) = open_store(root, project)?;
    let (segments, _) = segments(&store)?;
    let segment = &segments[segment_at(&segments, revision)?];
    let (state, sha) = catalog_at(segment, revision)?;
    let bytes = catalog_bytes(&state)?;
    if let Some(out) = out {
        // A new file only, so the authoritative catalog can never be overwritten from here.
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(out)
            .map_err(|e| format!("Cannot create {}: {e}", out.display()))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
    }
    Ok(json!({
        "backend": "store",
        "project": project,
        "revision": revision,
        "catalog_sha256": sha,
        "verified": true,
        "records": state.nodes.len(),
        "dependencies": state.dependencies.len(),
        "bytes": bytes.len(),
        "written_to": out.map(|p| p.display().to_string()),
    }))
}

/// Record `id` and its report as they were at `revision`.
pub fn show(root: &Path, project: &str, id: u64, revision: u64) -> Result<Value> {
    let (dir, store) = open_store(root, project)?;
    let (segments, _) = segments(&store)?;
    let at = segment_at(&segments, revision)?;
    let (state, _) = catalog_at(&segments[at], revision)?;
    let record = state.nodes.iter().find(|n| n.id == id);
    let path = record.map(|_| catalog_index::Index::new(&state).paths[&id].clone());
    // The report's text at `revision`: the last change at or before it in this segment, else
    // the first change after it while later segments continue this one without a gap.
    let entries = |segment: &Segment| -> Result<Vec<Value>> {
        segment
            .frames
            .iter()
            .skip(1)
            .filter(|range| mentions(&segment.bytes[(*range).clone()], id))
            .map(|range| {
                serde_json::from_slice(&segment.bytes[range.clone()]).map_err(|e| e.to_string())
            })
            .collect()
    };
    let earlier = entries(&segments[at])?;
    let mut sha = earlier
        .iter()
        .rev()
        .filter(|e| e["revision"].as_u64().is_some_and(|r| r <= revision))
        .find_map(|e| report_row(e, id))
        .map(|r| r["after_sha256"].clone());
    if sha.is_none() {
        for (i, segment) in segments.iter().enumerate().skip(at) {
            if i > at && segment.start["follows"] != "commit" {
                break;
            }
            let later = if i == at {
                earlier.clone()
            } else {
                entries(segment)?
            };
            sha = later
                .iter()
                .filter(|e| e["revision"].as_u64().is_some_and(|r| r > revision))
                .find_map(|e| report_row(e, id))
                .map(|r| r["before_sha256"].clone());
            if sha.is_some() {
                break;
            }
        }
    }
    let report = match (sha.as_ref().and_then(Value::as_str), &path) {
        (Some(sha), _) => {
            json!({"sha256": sha, "text": text_index(&store)?.read(sha)?, "source": "store"})
        }
        (None, Some(path)) => {
            // Unchanged in the store's history from this revision on: the file holds it.
            let text = fs::read_to_string(dir.join(path)).ok();
            json!({"sha256": text.as_deref().map(hash), "text": text, "source": "current_file"})
        }
        (None, None) => Value::Null,
    };
    Ok(json!({
        "backend": "store",
        "project": project,
        "revision": revision,
        "record": record,
        "path": path,
        "report": report,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_is_found_after_a_compaction_replaced_its_pack() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-workspaces")
            .join(format!("store-unit-{}", uuid::Uuid::new_v4()));
        let store = dir.join(FOLDER);
        {
            let mut w = Writer::open(&store).unwrap();
            // Two syncs' worth: two small frames, which compaction merges into a new pack.
            let first = w.text("first text").unwrap();
            w.flush_texts().unwrap();
            w.text("second text").unwrap();
            w.flush_texts().unwrap();
            let before = text_index(&store).unwrap();
            let compacted = compact_store(&store).unwrap();
            assert_eq!(compacted["small_frames_after"], 1, "{compacted}");
            // An index taken before the compaction still finds the text.
            assert_eq!(before.read(&first).unwrap().as_deref(), Some("first text"));
        }
        let _ = fs::remove_dir_all(&dir);
    }
}
