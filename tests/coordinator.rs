//! The coordinator end to end, through the `research` executable: snapshot reads, merged
//! writes, pending context, coexistence with direct mode, the handshake and graceful shutdown.
#![cfg(windows)]
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Command, Output},
    time::{Duration, Instant},
};

struct Fixture {
    home: PathBuf,
}
impl Fixture {
    /// A project with architecture 1, chunk 2 and `functions` functions (IDs 3 onwards).
    fn new(functions: u32) -> Self {
        let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-workspaces")
            .join(format!("coordinator-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&home).unwrap();
        let f = Self { home };
        f.direct(&[
            "project",
            "create",
            "fixture",
            "--name",
            "Fixture",
            "--binary",
            "synthetic",
        ]);
        f.direct(&[
            "node",
            "create",
            "fixture",
            "--parent",
            "0",
            "--name",
            "Architecture",
        ]);
        f.direct(&[
            "node", "create", "fixture", "--parent", "1", "--name", "Chunk",
        ]);
        let p = f.direct(&["project", "revision", "fixture"]);
        let request = json!({"schema": 2, "binary": p["binary"], "update": [], "create":
            (0..functions).map(|i| json!({"parent": 2, "name": format!("F{i}"), "rva": 0x1000 + 16 * i})).collect::<Vec<_>>()});
        let file = f.home.join("create.json");
        fs::write(&file, serde_json::to_vec(&request).unwrap()).unwrap();
        f.direct(&["node", "batch", "fixture", "--file", file.to_str().unwrap()]);
        f
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
    fn output(&self, mode: &str, args: &[&str]) -> Output {
        self.command(mode, args).output().unwrap()
    }
    fn ok(&self, mode: &str, args: &[&str]) -> Value {
        let o = self.output(mode, args);
        assert!(
            o.status.success(),
            "{mode} {args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        serde_json::from_slice(&o.stdout).unwrap()
    }
    fn direct(&self, args: &[&str]) -> Value {
        self.ok("off", args)
    }
    fn coordinated(&self, args: &[&str]) -> Value {
        self.ok("required", args)
    }
    fn save_args(&self, node: u64, token: &str, body: &str) -> Vec<String> {
        let body_file = self
            .home
            .join(format!("body-{node}-{}.md", uuid::Uuid::new_v4()));
        let evidence_file = self.home.join(format!("evidence-{node}.md"));
        fs::write(&body_file, body).unwrap();
        fs::write(&evidence_file, "Synthetic evidence").unwrap();
        vec![
            "node".into(),
            "save".into(),
            "fixture".into(),
            node.to_string(),
            "--expect".into(),
            token.into(),
            "--body-file".into(),
            body_file.to_string_lossy().into(),
            "--evidence-file".into(),
            evidence_file.to_string_lossy().into(),
        ]
    }
    fn receipts(&self) -> usize {
        fs::read_dir(
            self.home
                .join("projects/fixture/tools/workspaces/catalog-transactions-v4"),
        )
        .map(|d| {
            d.flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("revision-"))
                .count()
        })
        .unwrap_or(0)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.output("auto", &["coordinator", "stop"]);
        // The coordinator exits after draining; its lock file then becomes removable.
        let deadline = Instant::now() + Duration::from_secs(10);
        while fs::remove_dir_all(&self.home).is_err() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
fn without_coordinator_fields(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.remove("revision");
        object.remove("pending");
    }
    value
}

#[test]
fn reads_through_the_coordinator_match_direct_mode() {
    let f = Fixture::new(3);
    for args in [
        &["node", "show", "fixture", "3"][..],
        &["status", "fixture"],
        &["node", "list", "fixture", "--parent", "2"],
        &["review-queue", "fixture"],
        &["project", "revision", "fixture"],
    ] {
        let through = f.coordinated(args);
        if through.is_object() {
            assert_eq!(through["pending"], json!([]), "{args:?}");
            assert!(through["revision"].is_u64(), "{args:?}");
        }
        assert_eq!(
            without_coordinator_fields(through),
            without_coordinator_fields(f.direct(args)),
            "{args:?}"
        );
    }
    let status = f.coordinated(&["coordinator", "status"]);
    assert_eq!(status["running"], true);
    let counts = &status["projects"][0]["reads_by_class"];
    assert_eq!(counts["function"]["reads"], 1, "{status}");
    assert_eq!(counts["project"]["reads"], 4, "{status}");
    // Stopping writes the counts to the tooling log as one entry.
    f.coordinated(&["coordinator", "stop"]);
    let log = f
        .home
        .join("projects/fixture/tools/workspaces/history/tooling.jsonl");
    let deadline = Instant::now() + Duration::from_secs(20);
    while !fs::read_to_string(&log).is_ok_and(|t| t.contains("coordinator-reads")) {
        assert!(
            Instant::now() < deadline,
            "no coordinator-reads entry was written"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let entry: Value =
        serde_json::from_str(fs::read_to_string(&log).unwrap().lines().last().unwrap()).unwrap();
    assert_eq!(entry["diff"]["reads_by_class"]["function"]["reads"], 1);
}

#[test]
fn concurrent_saves_merge_into_fewer_commits_with_serial_results() {
    let f = Fixture::new(40);
    let tokens: Vec<String> = (3..43)
        .map(|id| {
            f.direct(&["node", "show", "fixture", &id.to_string()])["token"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    f.coordinated(&["queue", "fixture"]); // start the coordinator before the burst
    let before = f.receipts();
    let children: Vec<_> = (3..43u64)
        .zip(&tokens)
        .map(|(id, token)| {
            f.command("required", &[])
                .args(f.save_args(id, token, &format!("Research {id}")))
                .output()
        })
        .collect();
    // Saves were issued one after another above; now issue a concurrent burst.
    for child in children {
        assert!(child.unwrap().status.success());
    }
    let tokens: Vec<String> = (3..43)
        .map(|id| {
            f.direct(&["node", "show", "fixture", &id.to_string()])["token"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    let mid = f.receipts();
    let running: Vec<_> = (3..43u64)
        .zip(&tokens)
        .map(|(id, token)| {
            f.command("required", &[])
                .args(f.save_args(id, token, &format!("Revised {id}")))
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in running {
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    assert_eq!(mid - before, 40, "sequential saves commit one by one");
    let burst = f.receipts() - mid;
    assert!(burst < 40, "40 concurrent saves took {burst} commits");
    for id in 3..43u64 {
        assert_eq!(
            f.direct(&["node", "show", "fixture", &id.to_string()])["body"],
            format!("Revised {id}")
        );
    }
}

#[test]
fn reads_do_not_wait_for_a_blocked_writer_and_show_it_as_pending() {
    let f = Fixture::new(2);
    let token = f.direct(&["node", "show", "fixture", "3"])["token"]
        .as_str()
        .unwrap()
        .to_string();
    f.coordinated(&["queue", "fixture"]);
    // Another process holds the project's write lock, so the coordinator's writer must wait.
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(f.home.join("projects/.project-locks/fixture.lock"))
        .unwrap();
    lock.lock().unwrap();
    let save = f
        .command("required", &[])
        .args(f.save_args(3, &token, "Queued research"))
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(800));
    let started = Instant::now();
    let read = f.coordinated(&["node", "show", "fixture", "3"]);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "the read waited"
    );
    assert_eq!(read["body"], "", "the read answers from the last commit");
    assert_eq!(read["pending"][0]["kind"], "save");
    assert_eq!(read["pending"][0]["state"], "running");
    assert_eq!(read["pending"][0]["submitter"], "test");
    lock.unlock().unwrap();
    assert!(save.wait_with_output().unwrap().status.success());
    assert_eq!(
        f.coordinated(&["node", "show", "fixture", "3"])["body"],
        "Queued research"
    );
}

#[test]
fn reads_never_mix_revisions_while_the_same_report_is_rewritten() {
    let f = Fixture::new(1);
    f.coordinated(&["queue", "fixture"]);
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let home = f.home.clone();
    let readers: Vec<_> = (0..3)
        .map(|_| {
            let stop = stop.clone();
            let home = home.clone();
            std::thread::spawn(move || {
                let mut seen = vec![];
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let o = Command::new(env!("CARGO_BIN_EXE_research"))
                        .env("SCOPEWORK_COORDINATOR", "required")
                        .arg("--home")
                        .arg(&home)
                        .args(["node", "show", "fixture", "3"])
                        .output()
                        .unwrap();
                    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
                    seen.push((
                        v["revision"].as_u64().unwrap(),
                        v["body"].as_str().unwrap().to_string(),
                    ));
                }
                seen
            })
        })
        .collect();
    // The writer: each committed revision maps to exactly one body.
    let mut bodies = std::collections::BTreeMap::new();
    let start = f.direct(&["project", "revision", "fixture"])["revision"]
        .as_u64()
        .unwrap();
    bodies.insert(start, String::new());
    for round in 0..15 {
        let token = f.coordinated(&["node", "show", "fixture", "3"])["token"]
            .as_str()
            .unwrap()
            .to_string();
        let body = format!("Round {round}");
        let o = f
            .command("required", &[])
            .args(f.save_args(3, &token, &body))
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let revision = f.direct(&["project", "revision", "fixture"])["revision"]
            .as_u64()
            .unwrap();
        bodies.insert(revision, body);
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let mut checked = 0;
    for reader in readers {
        for (revision, body) in reader.join().unwrap() {
            let expected = bodies
                .range(..=revision)
                .next_back()
                .map(|(_, b)| b.clone())
                .unwrap();
            assert_eq!(
                body, expected,
                "revision {revision} answered with another revision's text"
            );
            checked += 1;
        }
    }
    assert!(checked > 0);
}

#[test]
fn direct_mode_commits_are_picked_up_by_the_next_read() {
    let f = Fixture::new(1);
    assert_eq!(f.coordinated(&["node", "show", "fixture", "3"])["body"], "");
    let token = f.direct(&["node", "show", "fixture", "3"])["token"]
        .as_str()
        .unwrap()
        .to_string();
    let o = f
        .command("off", &[])
        .args(f.save_args(3, &token, "Written directly"))
        .output()
        .unwrap();
    assert!(o.status.success());
    assert_eq!(
        f.coordinated(&["node", "show", "fixture", "3"])["body"],
        "Written directly"
    );
}

#[test]
fn the_handshake_turns_away_older_clients_and_retires_for_newer_ones() {
    let f = Fixture::new(0);
    f.coordinated(&["queue", "fixture"]);
    let name = research_dashboard::coordinator::pipe::pipe_name(&f.home).unwrap();
    let hello = |version: &str| -> Value {
        let pipe = research_dashboard::coordinator::pipe::connect(&name)
            .unwrap()
            .unwrap();
        let mut reader = BufReader::new(pipe.try_clone().unwrap());
        let mut writer = pipe;
        writeln!(
            writer,
            "{}",
            json!({"hello": {"protocol": 1, "version": version, "role": "cli"}})
        )
        .unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    };
    assert_eq!(hello("0.0.1")["hello"]["reason"], "older-client");
    assert_eq!(hello("999.0.0")["hello"]["reason"], "retiring");
    let deadline = Instant::now() + Duration::from_secs(10);
    while research_dashboard::coordinator::pipe::connect(&name)
        .unwrap()
        .is_some()
    {
        assert!(
            Instant::now() < deadline,
            "the retiring coordinator did not exit"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    // A client of this version starts a new coordinator; `status` itself never starts one.
    f.coordinated(&["queue", "fixture"]);
    assert_eq!(f.coordinated(&["coordinator", "status"])["running"], true);
}

#[test]
fn restart_manager_closes_the_coordinator_gracefully() {
    use windows_sys::Win32::System::RestartManager::{
        RmEndSession, RmRegisterResources, RmShutdown, RmStartSession,
    };
    let f = Fixture::new(0);
    // Run the coordinator from a private copy, so Restart Manager sees only this process.
    let exe = f.home.join("coordinator-copy.exe");
    fs::copy(env!("CARGO_BIN_EXE_research"), &exe).unwrap();
    let mut serve = Command::new(&exe);
    serve.arg("--home").arg(&f.home).arg("serve");
    {
        use std::os::windows::process::CommandExt;
        serve.creation_flags(0x0800_0000);
    }
    let mut child = serve.spawn().unwrap();
    let name = research_dashboard::coordinator::pipe::pipe_name(&f.home).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while research_dashboard::coordinator::pipe::connect(&name)
        .unwrap()
        .is_none()
    {
        assert!(Instant::now() < deadline, "the coordinator did not start");
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut session = 0u32;
    let mut key = [0u16; 64];
    let path: Vec<u16> = exe.to_string_lossy().encode_utf16().chain([0]).collect();
    let files = [path.as_ptr()];
    // SAFETY: Restart Manager calls with valid buffers; the session is ended below.
    let shutdown = unsafe {
        assert_eq!(RmStartSession(&mut session, 0, key.as_mut_ptr()), 0);
        assert_eq!(
            RmRegisterResources(
                session,
                1,
                files.as_ptr(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null()
            ),
            0
        );
        let result = RmShutdown(session, 0, None);
        RmEndSession(session);
        result
    };
    assert_eq!(shutdown, 0, "graceful shutdown failed");
    let status = child.wait().unwrap();
    assert!(status.success(), "the coordinator exited with {status:?}");
}

#[test]
fn gui_queue_polls_do_not_keep_an_idle_coordinator_alive() {
    let f = Fixture::new(0);
    f.coordinated(&["project", "revision", "fixture"]);
    let name = research_dashboard::coordinator::pipe::pipe_name(&f.home).unwrap();
    let deadline = Instant::now() + Duration::from_millis(2500);
    while Instant::now() < deadline {
        let pipe = research_dashboard::coordinator::pipe::connect(&name)
            .unwrap()
            .unwrap();
        let mut reader = BufReader::new(pipe.try_clone().unwrap());
        let mut writer = pipe;
        let version = env!("CARGO_PKG_VERSION");
        writeln!(
            writer,
            "{}",
            json!({"hello": {"protocol": 1, "version": version, "role": "gui"}})
        )
        .unwrap();
        writeln!(
            writer,
            "{}",
            json!({"id": 1, "args": ["queue", "fixture"], "class": "interactive"})
        )
        .unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        let answer: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(answer["stdout"]["source"], "coordinator", "{answer}");
        std::thread::sleep(Duration::from_millis(200));
    }
    let status = f.coordinated(&["coordinator", "status"]);
    assert!(status["idle_s"].as_u64().unwrap() >= 2, "{status}");
}

#[test]
fn a_failed_handshake_falls_back_to_direct_mode() {
    let f = Fixture::new(0);
    // A server that accepts each connection and hangs up before answering the hello.
    let name = research_dashboard::coordinator::pipe::pipe_name(&f.home).unwrap();
    let ready = std::sync::Arc::new(std::sync::Barrier::new(2));
    let started = ready.clone();
    std::thread::spawn(move || {
        let mut first = true;
        loop {
            let Ok(instance) = research_dashboard::coordinator::pipe::listen(&name, first) else {
                return;
            };
            if first {
                started.wait();
                first = false;
            }
            if research_dashboard::coordinator::pipe::accept(&instance).is_err() {
                return;
            }
            drop(instance);
        }
    });
    ready.wait();
    let direct = f.direct(&["project", "revision", "fixture"]);
    assert_eq!(f.ok("auto", &["project", "revision", "fixture"]), direct);
    let refused = f.output("required", &["project", "revision", "fixture"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("coordinator_unavailable"));
}
