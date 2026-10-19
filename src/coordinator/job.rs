//! `research job run`: runs a tool command while holding a lease on the project's tool
//! resources. Through a coordinator, same-key requests join one lease: the command receives
//! every joined payload and reports a result per payload. Without one, exclusive resource lock
//! files stand in, and the command only ever receives its own payload.
//!
//! The command's contract: read the JSON array named by `SCOPEWORK_JOB_PAYLOADS` (entries
//! `{"payload_id", "submitter", "kind", "payload"}`, this request's own first), do the work
//! once, and write `{"results": [{"payload_id", "ok"} or {"payload_id", "error"}], "overall":
//! ...}` to the path in `SCOPEWORK_JOB_RESULTS`. Each payload must get the result it would
//! have had alone, in order. A non-zero exit fails every payload.
use super::client;
use super::manifest::LeaseSpec;
use serde_json::{Value, json};
use std::path::Path;

/// Runs `command` under the lease `spec` describes. Returns the answer and the exit code.
pub fn run(
    home: &Path,
    project: &str,
    spec: &LeaseSpec,
    payload: Option<Value>,
    command: &[String],
) -> Result<(Value, i32), String> {
    let projects = home.join("projects");
    let dir = projects.join(project);
    if !dir.join("project.json").is_file() {
        return Err(format!("Unknown project: {project}"));
    }
    if command.is_empty() {
        return Err("Name the command to run after --".into());
    }
    let payload_given = payload.is_some();
    let answer = |source: &str, lease: Option<u64>, ran: bool, result: Value, exit: i32| {
        json!({"project": project, "kind": spec.kind, "resources": spec.resources, "source": source,
            "lease": lease, "ran": ran, "result": result, "exit": exit})
    };
    if let Some(mut connection) = client::connect(home, "tool", true)? {
        let request = json!({"id": 1, "lease": {"project": project, "resources": spec.resources,
            "key": spec.key, "class": spec.class, "kind": spec.kind, "payload": payload}});
        connection.post(&request)?;
        let reply = connection.receive()?;
        if let Some(error) = reply["error"].as_str() {
            return Err(error.to_string());
        }
        // Another holder did the work: `done` for this payload, `covered` without one.
        for field in ["done", "covered"] {
            if let Some(answered) = reply.get(field) {
                let result = answered["result"].clone();
                let exit = i32::from(result.get("error").is_some());
                return Ok((
                    answer(field, answered["by"].as_u64(), false, result, exit),
                    exit,
                ));
            }
        }
        if let Some(granted) = reply.get("granted") {
            let lease = granted["lease"].as_u64().ok_or("Malformed grant")?;
            let payloads: Vec<Value> = granted["payloads"].as_array().cloned().unwrap_or_default();
            let own = payloads
                .first()
                .and_then(|p| p["payload_id"].as_u64())
                .unwrap_or(lease);
            // The lease orders and batches; the resource lock files exclude, the same ones
            // direct mode takes, so a direct-mode job can never run beside a leased one.
            let locks = crate::research::resource_locks(&projects, project, &spec.resources);
            let (results, overall, exit) = match &locks {
                Ok(_) => execute(&dir, lease, &payloads, command)?,
                Err(e) => {
                    let error = format!("job_failed: {e}");
                    let results = payloads
                        .iter()
                        .map(|p| json!({"payload_id": p["payload_id"], "error": error}))
                        .collect();
                    (results, json!({"error": error}), 1)
                }
            };
            // Best effort: the work is done and this request's result is known. If reporting
            // fails, the lease still ends with this connection, and unanswered joined
            // requests wait again, as for any holder that is lost.
            let _ = connection.post(&json!({"id": 2, "results": results, "overall": overall}));
            let _ = connection.receive();
            let _ = connection.post(&json!({"id": 3, "release": lease}));
            let _ = connection.receive();
            let result = own_result(&results, own, payload_given, &overall);
            let exit = exit_code(exit, &result);
            return Ok((answer("coordinator", Some(lease), true, result, exit), exit));
        }
        // A stopping coordinator: run with lock files instead.
        if reply.get("unavailable").is_none() {
            return Err(format!("Unexpected answer from the coordinator: {reply}"));
        }
    }
    let _locks = crate::research::resource_locks(&projects, project, &spec.resources)?;
    let payloads = vec![
        json!({"payload_id": 1, "submitter": client::submitter(), "kind": spec.kind, "payload": payload}),
    ];
    let (results, overall, exit) = execute(&dir, 1, &payloads, command)?;
    let result = own_result(&results, 1, payload_given, &overall);
    let exit = exit_code(exit, &result);
    Ok((answer("locks", None, true, result, exit), exit))
}

/// A request's exit code, the same for a holder, a joined request and direct mode: the
/// command's own failure first, then whether this request's result is an error.
fn exit_code(command: i32, result: &Value) -> i32 {
    if command != 0 {
        command
    } else {
        i32::from(result.get("error").is_some())
    }
}

/// This request's own result: its payload's entry; a payload without one is answered
/// `no result`, as for a joined request. Without a payload, the overall result.
fn own_result(results: &[Value], own: u64, payload_given: bool, overall: &Value) -> Value {
    if payload_given {
        return results
            .iter()
            .find(|r| r["payload_id"].as_u64() == Some(own))
            .map(|r| {
                let mut r = r.clone();
                if let Some(object) = r.as_object_mut() {
                    object.remove("payload_id");
                }
                r
            })
            .filter(|r| r.get("ok").is_some() || r.get("error").is_some())
            .unwrap_or_else(|| json!({"error": "no result"}));
    }
    results
        .iter()
        .find(|r| r["payload_id"].as_u64() == Some(own))
        .map(|r| {
            let mut r = r.clone();
            if let Some(object) = r.as_object_mut() {
                object.remove("payload_id");
            }
            r
        })
        .unwrap_or_else(|| overall.clone())
}

/// Runs the command with the payload and result files; returns per-payload results, the
/// overall result and the exit code.
fn execute(
    dir: &Path,
    lease: u64,
    payloads: &[Value],
    command: &[String],
) -> Result<(Vec<Value>, Value, i32), String> {
    let scratch = dir.join("tools/scratch");
    std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
    let stem = format!("job-{}", uuid::Uuid::new_v4().simple());
    let payload_file = scratch.join(format!("{stem}-payloads.json"));
    let results_file = scratch.join(format!("{stem}-results.json"));
    std::fs::write(
        &payload_file,
        serde_json::to_vec(payloads).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    // The command's own output goes to stderr: stdout carries this command's JSON answer.
    let status = std::process::Command::new(&command[0])
        .args(&command[1..])
        .current_dir(dir)
        .env("SCOPEWORK_JOB_PAYLOADS", &payload_file)
        .env("SCOPEWORK_JOB_RESULTS", &results_file)
        .env("SCOPEWORK_JOB_LEASE", lease.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(std::io::stderr()))
        .status();
    let reported = std::fs::read(&results_file)
        .ok()
        .map(|bytes| serde_json::from_slice::<Value>(&bytes));
    let _ = std::fs::remove_file(&payload_file);
    let _ = std::fs::remove_file(&results_file);
    let exit = match status {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => return Err(format!("job_failed: could not start {}: {e}", command[0])),
    };
    if exit != 0 {
        let error = format!("job_failed: the command exited with {exit}");
        let results = payloads
            .iter()
            .map(|p| json!({"payload_id": p["payload_id"], "error": error}))
            .collect();
        return Ok((results, json!({"error": error}), exit));
    }
    match reported {
        None => Ok((vec![], json!({"exit": 0}), 0)),
        Some(Ok(value)) => Ok((
            value["results"].as_array().cloned().unwrap_or_default(),
            value.get("overall").cloned().unwrap_or(json!({"exit": 0})),
            0,
        )),
        Some(Err(e)) => {
            let error = format!("job_failed: the results file is not JSON ({e})");
            let results = payloads
                .iter()
                .map(|p| json!({"payload_id": p["payload_id"], "error": error}))
                .collect();
            Ok((results, json!({"error": error}), 1))
        }
    }
}

/// `research job status`: held and waiting leases, or the resource lock files without a
/// coordinator.
pub fn status(home: &Path, project: &str) -> Result<Value, String> {
    if let Some(mut connection) = client::connect(home, "cli", false)? {
        let answer = connection.request(&["queue".into(), project.into()], None)?;
        return match answer.stdout {
            Some(out) => {
                Ok(json!({"project": project, "source": "coordinator", "leases": out["leases"]}))
            }
            None => Err(answer
                .stderr
                .first()
                .and_then(|e| e["error"].as_str())
                .unwrap_or("The coordinator could not answer")
                .to_string()),
        };
    }
    let locks = crate::research::lock_status(&home.join("projects"), project)?;
    Ok(json!({"project": project, "source": "locks", "resources": locks["resources"]}))
}
