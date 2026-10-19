//! The client side: how the `research` executable and the GUI reach the coordinator.
//!
//! `SCOPEWORK_COORDINATOR` selects the behaviour: `off` always runs commands in this process
//! (direct mode); `auto` (the default) uses a coordinator, starting one if none runs, and
//! falls back to direct mode whenever one cannot be used; `required` fails instead.
use super::{Answer, Hello, PROTOCOL, VERSION, pipe};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Off,
    Auto,
    Required,
}
pub fn mode() -> Mode {
    match std::env::var("SCOPEWORK_COORDINATOR").as_deref() {
        Ok("off") => Mode::Off,
        Ok("required") => Mode::Required,
        _ => Mode::Auto,
    }
}
/// The name this client reports for its operations.
pub fn submitter() -> String {
    std::env::var("SCOPEWORK_SUBMITTER").unwrap_or_else(|_| {
        let command: Vec<String> = std::env::args().skip(1).take(3).collect();
        format!("pid {} ({})", std::process::id(), command.join(" "))
    })
}

/// An open, accepted connection.
pub struct Connection {
    reader: BufReader<std::fs::File>,
    writer: std::fs::File,
    next: u64,
}
impl Connection {
    fn send(&mut self, value: &Value) -> Result<Value, String> {
        writeln!(self.writer, "{value}").map_err(|e| e.to_string())?;
        self.writer.flush().map_err(|e| e.to_string())?;
        let mut line = String::new();
        if self
            .reader
            .read_line(&mut line)
            .map_err(|e| e.to_string())?
            == 0
        {
            return Err("The coordinator closed the connection".into());
        }
        serde_json::from_str(&line).map_err(|e| e.to_string())
    }
    /// Sends one command's arguments and returns its answer.
    pub fn request(&mut self, arguments: &[String], class: Option<&str>) -> Result<Answer, String> {
        self.next += 1;
        let mut request = json!({"id": self.next, "args": arguments});
        if let Some(class) = class {
            request["class"] = json!(class);
        }
        let reply = self.send(&request)?;
        serde_json::from_value(reply).map_err(|e| e.to_string())
    }
}

enum Handshake {
    Accepted(Connection),
    /// The coordinator is older and is retiring; start a new one once it is gone.
    Retiring,
    /// The coordinator is newer or incompatible; use direct mode.
    Refused,
}
fn handshake(pipe: std::fs::File, role: &str) -> Result<Handshake, String> {
    let reader = BufReader::new(pipe.try_clone().map_err(|e| e.to_string())?);
    let mut connection = Connection {
        reader,
        writer: pipe,
        next: 0,
    };
    let hello = Hello {
        protocol: PROTOCOL,
        version: VERSION.into(),
        build: 0,
        role: role.into(),
        submitter: submitter(),
    };
    let reply = connection.send(&json!({"hello": hello}))?;
    Ok(
        match (
            reply["hello"]["accepted"].as_bool(),
            reply["hello"]["reason"].as_str(),
        ) {
            (Some(true), _) => Handshake::Accepted(connection),
            (_, Some("retiring")) => Handshake::Retiring,
            _ => Handshake::Refused,
        },
    )
}
/// Starts a coordinator for `home` in the background, detached from this process.
fn spawn(home: &Path) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let executable = if executable
        .file_stem()
        .is_some_and(|s| s.eq_ignore_ascii_case("research"))
    {
        executable
    } else {
        executable.with_file_name(if cfg!(windows) {
            "research.exe"
        } else {
            "research"
        })
    };
    let mut command = std::process::Command::new(executable);
    command
        .arg("--home")
        .arg(home)
        .arg("serve")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .env_remove("SCOPEWORK_COORDINATOR");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No window, detached, its own process group: it outlives this command.
        command.creation_flags(0x0800_0000 | 0x0000_0008 | 0x0000_0200);
    }
    pipe::keep_standard_handles();
    command.spawn().map(|_| ()).map_err(|e| e.to_string())
}
/// Connects to `home`'s coordinator, starting one when allowed. `None` means direct mode.
pub fn connect(home: &Path, role: &str, start: bool) -> Result<Option<Connection>, String> {
    if mode() == Mode::Off {
        return Ok(None);
    }
    let name = match pipe::pipe_name(home) {
        Ok(name) => name,
        Err(e) if mode() == Mode::Required => return Err(e),
        Err(_) => return Ok(None),
    };
    let mut started = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match pipe::connect(&name) {
            Ok(Some(pipe)) => match handshake(pipe, role) {
                Ok(Handshake::Accepted(connection)) => return Ok(Some(connection)),
                Ok(Handshake::Refused) => {
                    return refuse("The coordinator runs a different version");
                }
                // Nothing was sent yet, so direct mode is safe.
                Err(e) => return refuse(&format!("The handshake failed: {e}")),
                Ok(Handshake::Retiring) => {
                    // Wait for the old coordinator to exit, then start ours.
                    std::thread::sleep(Duration::from_millis(100));
                }
            },
            Ok(None) => {
                if !start {
                    return Ok(None);
                }
                if !started {
                    spawn(home)?;
                    started = true;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(e) => return refuse(&e),
        }
        if Instant::now() >= deadline {
            return refuse("No coordinator answered in time");
        }
    }
}
fn refuse<T>(reason: &str) -> Result<Option<T>, String> {
    if mode() == Mode::Required {
        Err(format!("coordinator_unavailable: {reason}"))
    } else {
        Ok(None)
    }
}

/// `research coordinator status`.
pub fn status(home: &Path) -> Result<Value, String> {
    match connect(home, "cli", false)? {
        None => Ok(json!({"running": false})),
        Some(mut connection) => {
            let reply = connection.send(&json!({"id": 1, "status": true}))?;
            Ok(reply["stdout"].clone())
        }
    }
}
/// `research coordinator stop`.
pub fn stop(home: &Path) -> Result<Value, String> {
    match connect(home, "cli", false)? {
        None => Ok(json!({"running": false})),
        Some(mut connection) => {
            let _ = connection.send(&json!({"id": 1, "stop": true}));
            Ok(json!({"stopping": true}))
        }
    }
}
