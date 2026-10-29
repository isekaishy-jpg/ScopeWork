//! Restricted research pools (docs/proposal-research-pools.md): members work pooled records,
//! everyone else reads and builds on them, status stays the record's own, and quarantine
//! migrates into a pool.
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Output, time::Duration, time::Instant};

struct Fixture {
    home: PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.run_as("auto", "lead", &["coordinator", "stop"]);
        let deadline = Instant::now() + Duration::from_secs(10);
        while fs::remove_dir_all(&self.home).is_err() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
impl Fixture {
    /// Architecture 1 with chunk 2 (function 3, 0x1000) and chunk 4 (function 5, 0x2000).
    /// Chunk 2 is in pool `voice`, whose member is `astra`; `sol` is registered but not a member.
    fn new() -> Self {
        let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-workspaces")
            .join(format!("pools-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&home).unwrap();
        let f = Self { home };
        f.ok(
            "lead",
            &[
                "project",
                "create",
                "fixture",
                "--name",
                "Fixture",
                "--binary",
                "synthetic",
            ],
        );
        for (parent, name, rva) in [
            ("0", "Arch", None),
            ("1", "Voice", None),
            ("2", "F1", Some("1000")),
            ("1", "Other", None),
            ("4", "F2", Some("2000")),
        ] {
            let mut args = vec![
                "node", "create", "fixture", "--parent", parent, "--name", name,
            ];
            if let Some(rva) = rva {
                args.extend(["--rva", rva]);
            }
            f.ok("lead", &args);
        }
        for agent in ["astra", "sol"] {
            f.ok(agent, &["agent", "register"]);
        }
        f.ok(
            "lead",
            &[
                "pool",
                "add",
                "fixture",
                "voice",
                "2",
                "--origin",
                "safety-stop",
                "--reason",
                "Voice chat stops agents",
            ],
        );
        f.ok(
            "lead",
            &["pool", "join", "fixture", "voice", "--agent", "astra"],
        );
        f
    }
    fn run_as(&self, mode: &str, agent: &str, args: &[&str]) -> Output {
        std::process::Command::new(env!("CARGO_BIN_EXE_research"))
            .env("SCOPEWORK_COORDINATOR", mode)
            .env("SCOPEWORK_SUBMITTER", agent)
            .arg("--home")
            .arg(&self.home)
            .args(args)
            .output()
            .unwrap()
    }
    fn ok(&self, agent: &str, args: &[&str]) -> Value {
        let o = self.run_as("off", agent, args);
        assert!(
            o.status.success(),
            "{agent} {args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        serde_json::from_slice(&o.stdout).unwrap()
    }
    fn refused(&self, agent: &str, args: &[&str], kind: &str) {
        let o = self.run_as("off", agent, args);
        assert!(
            !o.status.success(),
            "{agent} {args:?} unexpectedly succeeded"
        );
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(err.contains(kind), "{agent} {args:?}: {err}");
    }
    fn show(&self, agent: &str, id: &str) -> Value {
        self.ok(agent, &["node", "show", "fixture", id])
    }
    fn save_args(&self, agent: &str, id: &str) -> Vec<String> {
        let b = self.home.join(format!("body-{id}.md"));
        let e = self.home.join(format!("evidence-{id}.md"));
        fs::write(&b, format!("Behaviour of {id}.")).unwrap();
        fs::write(&e, "Fixture evidence.").unwrap();
        let token = self.show(agent, id)["token"].as_str().unwrap().to_string();
        [
            "node",
            "save",
            "fixture",
            id,
            "--body-file",
            b.to_str().unwrap(),
            "--evidence-file",
            e.to_str().unwrap(),
            "--expect",
            &token,
        ]
        .map(String::from)
        .to_vec()
    }
    fn save(&self, agent: &str, id: &str) {
        let args = self.save_args(agent, id);
        self.ok(agent, &args.iter().map(String::as_str).collect::<Vec<_>>());
    }
    fn complete(&self, agent: &str, id: &str) {
        let token = self.show(agent, id)["completion_token"]
            .as_str()
            .unwrap()
            .to_string();
        self.ok(
            agent,
            &[
                "node",
                "complete",
                "fixture",
                id,
                "--expect",
                &token,
                "--reviewed",
            ],
        );
    }
    fn batch(&self, agent: &str, request: Value, directed: Option<&str>) -> Output {
        let path = self.home.join("batch.json");
        fs::write(&path, request.to_string()).unwrap();
        let mut args = vec!["node", "batch", "fixture", "--file", path.to_str().unwrap()];
        if let Some(d) = directed {
            args.extend(["--directed-by", d]);
        }
        self.run_as("off", agent, &args)
    }
    fn revision(&self) -> (Value, Value) {
        let p = self.ok("lead", &["project", "revision", "fixture"]);
        (p["binary"].clone(), p["revision"].clone())
    }
}

#[test]
fn members_work_pooled_records_and_others_are_refused() {
    let f = Fixture::new();
    // sol may not save, claim, flag, prove, set prerequisites of or complete function 3.
    let args = f.save_args("sol", "3");
    f.refused(
        "sol",
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
        "pooled",
    );
    f.refused(
        "sol",
        &["claim", "set", "fixture", "2", "--owner", "sol"],
        "pooled",
    );
    f.refused(
        "sol",
        &["node", "flag", "fixture", "3", "--reason", "check"],
        "pooled",
    );
    f.refused(
        "sol",
        &[
            "node",
            "proof",
            "fixture",
            "3",
            "--recreation",
            "a.rs",
            "--test",
            "t.rs",
        ],
        "pooled",
    );
    let structural = f.show("sol", "3")["structural_token"]
        .as_str()
        .unwrap()
        .to_string();
    f.refused(
        "sol",
        &[
            "deps",
            "set",
            "fixture",
            "3",
            "--expect",
            &structural,
            "2000",
        ],
        "pooled",
    );
    // astra, a member, does all of it.
    f.save("astra", "3");
    f.complete("astra", "3");
    assert_eq!(f.show("astra", "3")["effective_status"], "Complete");
    // A rename by sol needs the human's direction.
    let (binary, revision) = f.revision();
    let rename = json!({"schema":1,"binary":binary,"expect_revision":revision,
        "update":[{"id":3,"name":"Renamed"}]});
    assert!(!f.batch("sol", rename.clone(), None).status.success());
    let o = f.batch("sol", rename, Some("The user asked for this name"));
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn anyone_reads_pooled_records_and_builds_on_them() {
    let f = Fixture::new();
    f.save("astra", "3");
    f.complete("astra", "3");
    let shown = f.show("sol", "3");
    assert_eq!(shown["may_write"], false);
    assert_eq!(shown["pool"]["pool"], "voice");
    assert_eq!(shown["pool"]["node"], 2, "covered through its chunk");
    assert_eq!(shown["pool"]["members"], json!(["astra"]));
    assert_eq!(f.show("astra", "3")["may_write"], true);
    // sol names the pooled function as a prerequisite of its own work and completes it.
    let structural = f.show("sol", "5")["structural_token"]
        .as_str()
        .unwrap()
        .to_string();
    f.ok(
        "sol",
        &[
            "deps",
            "set",
            "fixture",
            "5",
            "--expect",
            &structural,
            "1000",
        ],
    );
    f.save("sol", "5");
    f.complete("sol", "5");
    assert_eq!(f.show("sol", "5")["effective_status"], "Complete");
}

#[test]
fn pooled_records_keep_their_status_and_are_open_work_only_for_members() {
    let f = Fixture::new();
    // Unlike quarantine, the pooled chunk is not complete without research.
    assert_eq!(f.show("lead", "2")["effective_status"], "NotStarted");
    assert_eq!(f.show("lead", "3")["effective_status"], "NotStarted");
    f.save("astra", "3");
    let ready = |agent: &str| -> Vec<u64> {
        f.ok(agent, &["status", "fixture", "--ready"])["records"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_u64().unwrap())
            .collect()
    };
    assert!(ready("astra").contains(&3));
    assert!(!ready("sol").contains(&3));
    let summary = f.ok("sol", &["status", "fixture", "--summary"]);
    assert_eq!(summary["pooled"], 2);
    let record = f.ok("sol", &["status", "fixture", "--parent", "2"])["records"][0].clone();
    assert_eq!(record["pool"], "voice");
}

#[test]
fn joining_needs_a_registered_agent() {
    let f = Fixture::new();
    f.refused(
        "lead",
        &["pool", "join", "fixture", "voice", "--agent", "nobody"],
        "agent_unregistered",
    );
    f.refused(
        "lead",
        &["pool", "join", "fixture", "nowhere", "--agent", "sol"],
        "No pool",
    );
    // Registering with no name registers the caller, which can then join itself.
    f.ok("kestrel", &["agent", "register"]);
    let names: Vec<String> = f.ok("lead", &["agent", "list"])["agents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"kestrel".to_string()));
    let joined = f.ok("kestrel", &["pool", "join", "fixture", "voice"]);
    assert_eq!(joined["members"], json!(["astra", "kestrel"]));
    f.save("kestrel", "3");
    f.ok("kestrel", &["pool", "leave", "fixture", "voice"]);
    // A real change is refused; re-saving the same text changes nothing, so it commits nothing.
    f.refused(
        "kestrel",
        &["node", "flag", "fixture", "3", "--reason", "check"],
        "pooled",
    );
}

#[test]
fn removal_and_prune_respect_pools_and_drop_their_records() {
    let f = Fixture::new();
    let request = f.home.join("delete.json");
    let delete = |agent: &str, ids: Value, prune: bool| -> Output {
        let (binary, revision) = f.revision();
        fs::write(
            &request,
            json!({"schema":1,"binary":binary,"expect_revision":revision,"ids":ids}).to_string(),
        )
        .unwrap();
        let verb = if prune { "prune" } else { "delete" };
        let preview = f.run_as(
            "off",
            agent,
            &["node", verb, "fixture", "--file", request.to_str().unwrap()],
        );
        // A refused preview, or one with nothing to remove, is the answer.
        let preview: Value = match serde_json::from_slice::<Value>(&preview.stdout) {
            Ok(value) if preview.status.success() && !value["token"].is_null() => value,
            _ => return preview,
        };
        f.run_as(
            "off",
            agent,
            &[
                "node",
                verb,
                "fixture",
                "--file",
                request.to_str().unwrap(),
                "--expect",
                preview["token"].as_str().unwrap(),
            ],
        )
    };
    assert!(
        !delete("sol", json!([3]), false).status.success(),
        "sol may not remove it"
    );
    let o = delete("astra", json!([3]), false);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    // The chunk is now empty; sol's prune passes over it, astra's removes it with its record.
    let pruned = delete("sol", json!([2]), true);
    assert!(
        pruned.status.success(),
        "{}",
        String::from_utf8_lossy(&pruned.stderr)
    );
    let pruned: Value = serde_json::from_slice(&pruned.stdout).unwrap();
    assert_eq!(pruned["excluded"][0]["id"], 2, "{pruned}");
    assert_eq!(pruned["excluded"][0]["reason"], "pooled", "{pruned}");
    let o = delete("astra", json!([2]), true);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let pools = f.ok("lead", &["pool", "list", "fixture"]);
    assert_eq!(pools["pools"][0]["records"], json!([]));
}

#[test]
fn quarantine_migrates_into_a_pool_and_blocks_writes_until_then() {
    let f = Fixture::new();
    let dir = f.home.join("projects/fixture");
    fs::write(
        dir.join("quarantine.json"),
        json!({"schema":1,"records":{"4":{"origin":"cyber-stop","reason":"Network code",
            "at_revision":3,"by":"astra"}}})
        .to_string(),
    )
    .unwrap();
    f.refused(
        "astra",
        &["node", "flag", "fixture", "5", "--reason", "x"],
        "quarantine_unmigrated",
    );
    let migrated = f.ok("lead", &["pool", "migrate-quarantine", "fixture"]);
    assert_eq!(migrated["migrated"], json!([4]));
    // A second migration finds nothing to move and commits nothing.
    let revision = f.ok("lead", &["project", "revision", "fixture"])["revision"].clone();
    let again = f.ok("lead", &["pool", "migrate-quarantine", "fixture"]);
    assert_eq!(again["migrated"], json!([]));
    assert_eq!(
        f.ok("lead", &["project", "revision", "fixture"])["revision"],
        revision
    );
    assert_eq!(fs::read_to_string(dir.join("quarantine.json")).unwrap(), "");
    let shown = f.show("sol", "5");
    assert_eq!(shown["pool"]["pool"], "quarantine");
    assert_eq!(shown["pool"]["origin"], "safety-stop");
    assert_eq!(shown["pool"]["reason"], "Network code");
    // Writes work again, and the pool decides who may change the migrated records.
    f.ok("sol", &["node", "flag", "fixture", "1", "--reason", "x"]);
    f.refused(
        "astra",
        &["node", "flag", "fixture", "5", "--reason", "x"],
        "pooled",
    );
    let history = String::from_utf8_lossy(
        &f.run_as("off", "lead", &["history", "show", "fixture"])
            .stdout,
    )
    .into_owned();
    assert!(history.contains("pools.json"), "{history}");
}

#[test]
fn an_unreadable_pools_file_refuses_writes_and_keeps_reads() {
    let f = Fixture::new();
    fs::write(f.home.join("projects/fixture/pools.json"), "{not json").unwrap();
    f.refused(
        "astra",
        &["node", "flag", "fixture", "5", "--reason", "x"],
        "pools_unreadable",
    );
    assert_eq!(f.show("sol", "3")["effective_status"], "NotStarted");
}

#[test]
fn pools_check_each_callers_name_through_the_coordinator() {
    let f = Fixture::new();
    let args = f.save_args("astra", "3");
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let o = f.run_as("required", "astra", &args);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let o = f.run_as(
        "required",
        "sol",
        &["node", "flag", "fixture", "3", "--reason", "x"],
    );
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("pooled"));
    let o = f.run_as(
        "required",
        "astra",
        &["node", "flag", "fixture", "3", "--reason", "x"],
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn only_members_take_records_out_of_or_move_them_between_pools() {
    let f = Fixture::new();
    f.refused(
        "sol",
        &["pool", "remove", "fixture", "2", "--reason", "mine now"],
        "pooled",
    );
    f.refused(
        "sol",
        &[
            "pool", "add", "fixture", "mine", "2", "--origin", "human", "--reason", "mine",
        ],
        "pooled",
    );
    // Pooling a record that is in no pool stays open to everyone.
    f.ok(
        "sol",
        &[
            "pool",
            "add",
            "fixture",
            "network",
            "4",
            "--origin",
            "safety-stop",
            "--reason",
            "stop",
        ],
    );
    // A member updates the reason in place, then moves the chunk to another pool.
    f.ok(
        "astra",
        &[
            "pool",
            "add",
            "fixture",
            "voice",
            "2",
            "--origin",
            "safety-stop",
            "--reason",
            "Codec code",
        ],
    );
    f.ok(
        "astra",
        &[
            "pool",
            "add",
            "fixture",
            "codec",
            "2",
            "--origin",
            "human",
            "--reason",
            "Split out",
        ],
    );
    let events: Vec<(String, String)> = serde_json::from_slice::<Value>(
        &fs::read(f.home.join("projects/fixture/pools.json")).unwrap(),
    )
    .unwrap()["log"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["node"] == 2)
        .map(|e| {
            (
                e["event"].as_str().unwrap().to_string(),
                e["pool"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let expected: Vec<(String, String)> = [
        ("add", "voice"),
        ("update", "voice"),
        ("remove", "voice"),
        ("add", "codec"),
    ]
    .iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect();
    assert_eq!(events, expected);
    // The human's direction overrides.
    f.ok(
        "sol",
        &[
            "pool",
            "remove",
            "fixture",
            "2",
            "--reason",
            "Reopened",
            "--directed-by",
            "The user reopened voice research",
        ],
    );
    assert!(f.show("sol", "3")["pool"].is_null());
}

#[test]
fn a_function_covered_by_its_chunk_cannot_be_moved_to_another_pool_by_a_non_member() {
    let f = Fixture::new();
    f.refused(
        "sol",
        &[
            "pool", "add", "fixture", "mine", "3", "--origin", "human", "--reason", "mine",
        ],
        "pooled",
    );
    assert_eq!(f.show("sol", "3")["pool"]["pool"], "voice");
    // A member may give it an entry of its own.
    f.ok(
        "astra",
        &[
            "pool",
            "add",
            "fixture",
            "narrow",
            "3",
            "--origin",
            "human",
            "--reason",
            "Codec only",
        ],
    );
    assert_eq!(f.show("sol", "3")["pool"]["pool"], "narrow");
}

#[test]
fn an_import_into_a_chunk_pooled_after_its_reservation_needs_a_member_or_direction() {
    let f = Fixture::new();
    let start = |directed: Option<&str>, rva: u64| -> String {
        let p = f.ok("sol", &["project", "show", "fixture"]);
        let mut batch = json!({"schema":1,"binary":p["binary"],"expect_revision":p["revision"],
            "create":[{"parent":4,"name":format!("Imported {rva:x}"),"rva":rva}]});
        if let Some(d) = directed {
            batch["directed_by"] = json!(d);
        }
        let file = f.home.join(format!("import-{rva:x}.json"));
        fs::write(&file, batch.to_string()).unwrap();
        let job = f.ok(
            "sol",
            &[
                "import",
                "start",
                "fixture",
                "--file",
                file.to_str().unwrap(),
                "--paused",
            ],
        );
        job["id"].as_str().unwrap().to_string()
    };
    let job = start(None, 0x9000);
    // The chunk is pooled after the reservation; pooling changes no structure.
    f.ok(
        "lead",
        &[
            "pool",
            "add",
            "fixture",
            "late",
            "4",
            "--origin",
            "human",
            "--reason",
            "Pooled late",
        ],
    );
    let o = f.run_as("off", "sol", &["import", "resume", "fixture", &job]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(text.contains("pooled"), "{text}");
    let created = |rva: &str| {
        f.ok("lead", &["project", "show", "fixture"])["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["rva"] == u64::from_str_radix(rva, 16).unwrap())
    };
    assert!(!created("9000"), "nothing was published");
    f.ok("sol", &["import", "cancel", "fixture", &job]);
    // With the human's direction the same import publishes.
    let job = start(Some("The user asked for these records"), 0xA000);
    let done = f.ok("sol", &["import", "resume", "fixture", &job]);
    assert_eq!(done["state"], "Committed", "{done}");
    assert!(created("a000"));
}

#[test]
fn adding_a_pools_records_again_amends_its_reason_for_members_only() {
    let f = Fixture::new();
    let reason = |f: &Fixture| {
        f.ok("lead", &["pool", "list", "fixture"])["pools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["pool"] == "voice")
            .unwrap()["reason"]
            .clone()
    };
    let amend = [
        "pool",
        "add",
        "fixture",
        "voice",
        "2",
        "--origin",
        "safety-stop",
        "--reason",
    ];
    // A non-member cannot amend it: the records it names are pooled.
    let mut args = amend.to_vec();
    args.push("Wrong");
    f.refused("sol", &args, "pooled");
    assert_eq!(reason(&f), "Voice chat stops agents");
    // A member re-adds the same records with a corrected reason: the pool's own reason follows,
    // also when the records already carry it.
    let mut args = amend.to_vec();
    args.push("Voice chat: shared by astra and sol");
    let added = f.ok("astra", &args);
    assert_eq!(
        added["pool_reason"], "Voice chat: shared by astra and sol",
        "{added}"
    );
    assert_eq!(reason(&f), "Voice chat: shared by astra and sol");
    // Adding a new record with another reason leaves the pool's reason as it is.
    f.ok(
        "astra",
        &[
            "pool",
            "add",
            "fixture",
            "voice",
            "4",
            "--origin",
            "human",
            "--reason",
            "Also this",
        ],
    );
    assert_eq!(reason(&f), "Voice chat: shared by astra and sol");
}
