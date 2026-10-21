//! The GUI's crash log. A windowed process has nowhere to send standard error, so the standard
//! library's own failure messages (an allocation failure, a panic) were lost. At startup the GUI
//! points standard error at `logs/gui.log` in the research home and records panics there with
//! a backtrace. The log is bounded: over [`LIMIT`] it is rotated to `gui.log.1` at the next start.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// Size above which the log is rotated when the GUI starts.
pub const LIMIT: u64 = 1024 * 1024;

/// The log file inside a research home.
pub fn path(home: &Path) -> PathBuf {
    home.join("logs").join("gui.log")
}

/// Opens the log for this run (rotating an oversized one), points standard error at it and
/// installs the panic hook. Returns the log path, or `None` when the log cannot be opened, in
/// which case the GUI runs without it.
pub fn install(home: &Path) -> Option<PathBuf> {
    let path = path(home);
    let file = open(&path).ok()?;
    redirect_stderr(file);
    std::panic::set_hook(Box::new(|info| {
        let thread = std::thread::current();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-text panic payload".into());
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".into());
        let record = format!(
            "{} panic in thread '{}' at {location}: {message}\nbacktrace:\n{}\n",
            timestamp(SystemTime::now()),
            thread.name().unwrap_or("unnamed"),
            std::backtrace::Backtrace::force_capture()
        );
        // Standard error is the log after `install`; a failed write has nowhere better to go.
        let _ = io::stderr().write_all(record.as_bytes());
    }));
    Some(path)
}

/// Rotates an oversized log, opens it for appending and writes this run's header line.
pub fn open(path: &Path) -> io::Result<File> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    // Another running GUI may hold the log open; then this run appends instead of rotating.
    let _ = rotate(path);
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(header(SystemTime::now()).as_bytes())?;
    Ok(file)
}

/// Moves a log larger than [`LIMIT`] to `<name>.1`, replacing the previous rotation.
pub fn rotate(path: &Path) -> io::Result<()> {
    match fs::metadata(path) {
        Ok(meta) if meta.len() > LIMIT => {
            let mut rotated = path.as_os_str().to_owned();
            rotated.push(".1");
            let rotated = PathBuf::from(rotated);
            let _ = fs::remove_file(&rotated);
            fs::rename(path, &rotated)
        }
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// One line identifying the run: time, product and build, process ID.
pub fn header(now: SystemTime) -> String {
    format!(
        "--- {} start: {} (pid {})\n",
        timestamp(now),
        crate::build_info::DISPLAY,
        std::process::id()
    )
}

/// UTC time as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn timestamp(now: SystemTime) -> String {
    let seconds = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

#[cfg(windows)]
fn redirect_stderr(file: File) {
    use std::os::windows::io::IntoRawHandle;
    use windows_sys::Win32::System::Console::{STD_ERROR_HANDLE, SetStdHandle};
    // The handle stays open for the life of the process: standard error now refers to it.
    let handle = file.into_raw_handle();
    // SAFETY: a valid, owned file handle that is never closed afterwards.
    unsafe {
        SetStdHandle(STD_ERROR_HANDLE, handle as _);
    }
}

#[cfg(not(windows))]
fn redirect_stderr(file: File) {
    // Other platforms keep their inherited standard error; the panic hook still writes there.
    drop(file);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn timestamps_are_utc_calendar_times() {
        assert_eq!(timestamp(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        let leap_day = UNIX_EPOCH + Duration::from_secs(951_782_400 + 3_661);
        assert_eq!(timestamp(leap_day), "2000-02-29T01:01:01Z");
        let later = UNIX_EPOCH + Duration::from_secs(1_792_540_800);
        assert_eq!(timestamp(later), "2026-10-21T00:00:00Z");
    }

    #[test]
    fn an_oversized_log_rotates_and_a_small_one_stays() {
        let folder = std::env::temp_dir().join(format!("crash-log-{}", uuid::Uuid::new_v4()));
        let log = folder.join("logs/gui.log");
        let first = open(&log).unwrap();
        drop(first);
        let size = fs::metadata(&log).unwrap().len();
        open(&log).unwrap();
        assert!(
            fs::metadata(&log).unwrap().len() > size,
            "a small log is appended to"
        );
        fs::write(&log, vec![b'x'; (LIMIT + 1) as usize]).unwrap();
        open(&log).unwrap();
        let rotated = folder.join("logs/gui.log.1");
        assert_eq!(fs::metadata(&rotated).unwrap().len(), LIMIT + 1);
        let fresh = fs::read_to_string(&log).unwrap();
        assert!(
            fresh.starts_with("--- ") && fresh.contains("start:"),
            "{fresh}"
        );
        fs::remove_dir_all(&folder).unwrap();
    }
}
