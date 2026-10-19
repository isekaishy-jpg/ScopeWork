//! Phase 2: leases on tool resources through `research job run`, with a stand-in tool.
#![cfg(windows)]
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

/// The stand-in tool: logs `start`/`end` lines with the payload count, answers each payload
/// with itself, sleeps for args[1] ms, and exits with args[2] (default 0).
const TOOL: &str = r#"
param([string]$Log, [int]$SleepMs = 0, [int]$Exit = 0, [string]$Write = "")
# Windows PowerShell emits a JSON array as one object: enumerate it explicitly.
$payloads = @((Get-Content -Raw $env:SCOPEWORK_JOB_PAYLOADS | ConvertFrom-Json) | ForEach-Object { $_ })
Add-Content -Path $Log -Value ("start " + $payloads.Count + " " + [DateTime]::UtcNow.Ticks)
if ($Write -ne "") { Set-Content -Path $Write -Value ("written " + [DateTime]::UtcNow.Ticks) }
Start-Sleep -Milliseconds $SleepMs
Add-Content -Path $Log -Value ("end " + $payloads.Count + " " + [DateTime]::UtcNow.Ticks)
if ($Exit -ne 0) { exit $Exit }
$results = @($payloads | ForEach-Object { @{ payload_id = $_.payload_id; ok = $_.payload } })
$answer = @{ results = $results; overall = @{ count = $payloads.Count } }
$answer | ConvertTo-Json -Depth 6 | Set-Content -Path $env:SCOPEWORK_JOB_RESULTS
"#;

struct Fixture {
    home: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-workspaces")
            .join(format!("leases-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&home).unwrap();
        let f = Self { home };
        let o = f
            .command(
                "off",
                &[
                    "project",
                    "create",
                    "fixture",
                    "--name",
                    "Fixture",
                    "--binary",
                    "synthetic",
                ],
            )
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        fs::write(f.home.join("tool.ps1"), TOOL).unwrap();
        fs::create_dir_all(f.dir().join("tools/profiles")).unwrap();
        fs::write(
            f.dir().join("tools/coordinator.toml"),
            "version = 1\n[resources.ledger]\nfiles = [\"tools/profiles/annotations.json\"]\n[kinds.ledger-put]\nresources = [\"ledger\"]\nkey = \"ledger-put\"\n",
        )
        .unwrap();
        f
    }
    fn dir(&self) -> PathBuf {
        self.home.join("projects/fixture")
    }
    fn log(&self) -> PathBuf {
        self.home.join("runs.log")
    }
    fn command(&self, mode: &str, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_research"));
        c.env("SCOPEWORK_COORDINATOR", mode)
            .env("SCOPEWORK_SUBMITTER", "test")
            .arg("--home")
            .arg(&self.home)
            .args(args);
        c
    }
    /// `job run fixture <selectors> -- powershell tool.ps1 <log> <sleep> <exit> [<write>]`.
    fn job(
        &self,
        mode: &str,
        selectors: &[&str],
        payload: Option<Value>,
        sleep_ms: u32,
        exit: i32,
        write: Option<&str>,
    ) -> Command {
        let mut args: Vec<String> = vec!["job".into(), "run".into(), "fixture".into()];
        args.extend(selectors.iter().map(|s| s.to_string()));
        if let Some(payload) = payload {
            let file = self
                .home
                .join(format!("payload-{}.json", uuid::Uuid::new_v4()));
            fs::write(&file, payload.to_string()).unwrap();
            args.push("--payload-file".into());
            args.push(file.to_string_lossy().into());
        }
        args.extend(
            [
                "--",
                "powershell",
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ]
            .map(String::from),
        );
        args.push(self.home.join("tool.ps1").to_string_lossy().into());
        args.push(self.log().to_string_lossy().into());
        args.push(sleep_ms.to_string());
        args.push(exit.to_string());
        if let Some(write) = write {
            args.push(self.dir().join(write).to_string_lossy().into());
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let mut c = self.command(mode, &refs);
        c.stdout(Stdio::piped()).stderr(Stdio::piped());
        c
    }
    fn answer(output: Output) -> (Value, i32) {
        let code = output.status.code().unwrap_or(-1);
        let value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|_| panic!("exit {code}: {}", String::from_utf8_lossy(&output.stderr)));
        (value, code)
    }
    fn runs(&self) -> Vec<(String, u64, u64)> {
        fs::read_to_string(self.log())
            .unwrap_or_default()
            .lines()
            .map(|l| {
                let parts: Vec<&str> = l.split_whitespace().collect();
                (
                    parts[0].to_string(),
                    parts[1].parse().unwrap(),
                    parts[2].parse().unwrap(),
                )
            })
            .collect()
    }
    /// Waits until the coordinator holds a lease on `resource`.
    fn wait_held(&self, resource: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let o = self
                .command("required", &["job", "status", "fixture"])
                .output()
                .unwrap();
            let status: Value = serde_json::from_slice(&o.stdout).unwrap_or(Value::Null);
            let held = status["leases"]["held"].as_array().is_some_and(|h| {
                h.iter().any(|l| {
                    l["resources"]
                        .as_array()
                        .is_some_and(|r| r.iter().any(|x| x == resource))
                })
            });
            if held {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "no lease on {resource} was held: {status}"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    fn waiting(&self) -> usize {
        let o = self
            .command("required", &["job", "status", "fixture"])
            .output()
            .unwrap();
        let status: Value = serde_json::from_slice(&o.stdout).unwrap_or(Value::Null);
        status["leases"]["waiting"].as_array().map_or(0, Vec::len)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.command("auto", &["coordinator", "stop"]).output();
        let deadline = Instant::now() + Duration::from_secs(10);
        while fs::remove_dir_all(&self.home).is_err() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
fn spawn(mut c: Command) -> Child {
    c.spawn().unwrap()
}

#[test]
fn queued_requests_join_one_run_and_each_gets_its_own_result() {
    let f = Fixture::new();
    let blocker = spawn(f.job(
        "required",
        &["--kind", "ledger-put"],
        Some(json!({"n": 0})),
        2500,
        0,
        None,
    ));
    f.wait_held("ledger");
    let joiners: Vec<Child> = (1..=3)
        .map(|n| {
            spawn(f.job(
                "required",
                &["--kind", "ledger-put"],
                Some(json!({"n": n})),
                0,
                0,
                None,
            ))
        })
        .collect();
    let queued = |n: usize| {
        let deadline = Instant::now() + Duration::from_secs(20);
        while f.waiting() < n {
            assert!(Instant::now() < deadline, "the requests did not queue");
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    queued(3);
    // Queued last, a payload-less request with the same key is covered by the same run.
    let covered = spawn(f.job("required", &["--kind", "ledger-put"], None, 0, 0, None));
    queued(4);
    let (first, code) = Fixture::answer(blocker.wait_with_output().unwrap());
    assert_eq!(
        (code, &first["result"]),
        (0, &json!({"ok": {"n": 0}})),
        "{first}"
    );
    let answers: Vec<Value> = joiners
        .into_iter()
        .map(|c| Fixture::answer(c.wait_with_output().unwrap()).0)
        .collect();
    for (n, answer) in (1..=3).zip(&answers) {
        assert_eq!(
            answer["result"],
            json!({"ok": {"n": n}}),
            "{answers:?} {:?}",
            f.runs()
        );
    }
    // Exactly one of them ran, for all three payloads; the others were answered by it.
    let ran: Vec<&Value> = answers.iter().filter(|a| a["ran"] == true).collect();
    assert_eq!(ran.len(), 1, "{answers:?}");
    let lease = ran[0]["lease"].clone();
    assert!(
        answers
            .iter()
            .filter(|a| a["ran"] == false)
            .all(|a| a["source"] == "done" && a["lease"] == lease)
    );
    let (cover, _) = Fixture::answer(covered.wait_with_output().unwrap());
    assert_eq!(
        (cover["source"].as_str(), &cover["result"]),
        (Some("covered"), &json!({"count": 3})),
        "{cover}"
    );
    let starts: Vec<u64> = f
        .runs()
        .iter()
        .filter(|r| r.0 == "start")
        .map(|r| r.1)
        .collect();
    assert_eq!(
        starts,
        [1, 3],
        "two runs: the blocker alone, then three payloads together"
    );
}

#[test]
fn holders_never_overlap_and_a_failed_command_fails_its_request() {
    let f = Fixture::new();
    let a = spawn(f.job("required", &["--resource", "ghidra"], None, 600, 0, None));
    let b = spawn(f.job("required", &["--resource", "ghidra"], None, 600, 3, None));
    let (a, a_code) = Fixture::answer(a.wait_with_output().unwrap());
    let (b, b_code) = Fixture::answer(b.wait_with_output().unwrap());
    assert_eq!(a_code + b_code, 3, "{a} {b}");
    let failed = if a_code == 3 { &a } else { &b };
    assert!(
        failed["result"]["error"]
            .as_str()
            .unwrap()
            .starts_with("job_failed"),
        "{failed}"
    );
    let runs = f.runs();
    assert_eq!(runs.len(), 4);
    // start, end, start, end: the second run started after the first ended.
    assert_eq!(
        runs.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
        ["start", "end", "start", "end"]
    );
    assert!(runs[2].2 >= runs[1].2);
}

#[test]
fn a_dead_holder_releases_its_lease_and_its_joined_request_runs() {
    let f = Fixture::new();
    let mut holder = spawn(f.job(
        "required",
        &["--kind", "ledger-put"],
        Some(json!({"n": 1})),
        6_000,
        0,
        None,
    ));
    f.wait_held("ledger");
    let joined = spawn(f.job(
        "required",
        &["--kind", "ledger-put"],
        Some(json!({"n": 2})),
        0,
        0,
        None,
    ));
    let deadline = Instant::now() + Duration::from_secs(20);
    while f.waiting() < 1 {
        assert!(
            Instant::now() < deadline,
            "the second request did not queue"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    holder.kill().unwrap();
    let _ = holder.wait();
    let (answer, code) = Fixture::answer(joined.wait_with_output().unwrap());
    assert_eq!((code, answer["ran"].as_bool()), (0, Some(true)), "{answer}");
    assert_eq!(answer["result"], json!({"ok": {"n": 2}}));
}

#[test]
fn without_a_coordinator_lock_files_exclude_and_show_in_locks() {
    let f = Fixture::new();
    let running = spawn(f.job(
        "off",
        &["--kind", "ledger-put"],
        Some(json!({"n": 1})),
        1500,
        0,
        None,
    ));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let o = f.command("off", &["locks", "fixture"]).output().unwrap();
        let locks: Value = serde_json::from_slice(&o.stdout).unwrap();
        if locks["resources"]["ledger"]["writer_active"] == true {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "no resource lock appeared: {locks}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let second = spawn(f.job(
        "off",
        &["--kind", "ledger-put"],
        Some(json!({"n": 2})),
        0,
        0,
        None,
    ));
    let (first, _) = Fixture::answer(running.wait_with_output().unwrap());
    let (second, _) = Fixture::answer(second.wait_with_output().unwrap());
    assert_eq!(
        (first["source"].as_str(), &first["result"]),
        (Some("locks"), &json!({"ok": {"n": 1}}))
    );
    assert_eq!(
        second["result"],
        json!({"ok": {"n": 2}}),
        "no merging without a coordinator"
    );
    let starts: Vec<u64> = f
        .runs()
        .iter()
        .filter(|r| r.0 == "start")
        .map(|r| r.1)
        .collect();
    assert_eq!(starts, [1, 1]);
    // Released locks leave no holder record behind.
    let holder = f
        .home
        .join("projects/.project-locks/fixture.res-ledger.holder");
    assert!(!holder.exists(), "stale holder record");
}

#[test]
fn changes_outside_a_lease_are_recorded_as_drift() {
    let f = Fixture::new();
    let ledger = f.dir().join("tools/profiles/annotations.json");
    fs::write(&ledger, "{}").unwrap();
    // Any coordinated command opens the project and starts its drift watch.
    let o = f
        .command("required", &["project", "revision", "fixture"])
        .output()
        .unwrap();
    assert!(o.status.success());
    std::thread::sleep(Duration::from_millis(2500));
    let history = f.dir().join("tools/workspaces/history/tooling.jsonl");
    let drift_entries = || {
        fs::read_to_string(&history)
            .unwrap_or_default()
            .lines()
            .filter(|l| l.contains("\"entry_kind\":\"drift\""))
            .count()
    };
    // Scopework's own catalog writes without a transaction receipt (here the research scope)
    // are not drift.
    let revision = f
        .command("off", &["project", "revision", "fixture"])
        .output()
        .unwrap();
    let revision: Value = serde_json::from_slice(&revision.stdout).unwrap();
    let o = f
        .command(
            "off",
            &[
                "project",
                "set-scope",
                "fixture",
                "--scope",
                "Rendering",
                "--expect-revision",
                &revision["revision"].to_string(),
            ],
        )
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    // Written under a lease: the holder's change.
    let (answer, code) = Fixture::answer(
        f.job(
            "required",
            &["--kind", "ledger-put"],
            None,
            0,
            0,
            Some("tools/profiles/annotations.json"),
        )
        .output()
        .unwrap(),
    );
    assert_eq!(code, 0, "{answer}");
    std::thread::sleep(Duration::from_millis(5000));
    assert_eq!(drift_entries(), 0, "a leased write is not drift");
    // Written by hand: drift, once.
    fs::write(&ledger, "{\"edited\": true}").unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while drift_entries() == 0 {
        assert!(Instant::now() < deadline, "no drift entry was recorded");
        std::thread::sleep(Duration::from_millis(200));
    }
    std::thread::sleep(Duration::from_millis(4500));
    assert_eq!(drift_entries(), 1);
    let entry: Value = fs::read_to_string(&history)
        .unwrap()
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|e| e["entry_kind"] == "drift")
        .unwrap();
    assert_eq!(entry["diff"]["resource"], "ledger");
    assert_eq!(entry["diff"]["file"], "tools/profiles/annotations.json");
    let status = f
        .command("required", &["coordinator", "status"])
        .output()
        .unwrap();
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["projects"][0]["drift"]["ledger"], 1, "{status}");
}
