//! Registered agents (docs/proposal-research-pools.md): the names agents work under in this
//! research home, so they can join research pools. An agent registers itself; nothing approves
//! it. `agents.json` in the research home has its own lock and is replaced atomically.
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

const FILE: &str = "agents.json";
const LOCK: &str = ".agents.lock";
/// The longest agent name, in characters.
const NAME_MAX: usize = 60;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Agents {
    schema: u32,
    #[serde(default)]
    agents: Vec<Agent>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Agent {
    pub name: String,
    pub registered_unix: u64,
}

fn valid(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > NAME_MAX || name.chars().any(char::is_control) {
        return Err(format!(
            "An agent name is 1 to {NAME_MAX} characters on one line, as SCOPEWORK_SUBMITTER gives it."
        ));
    }
    Ok(name.into())
}
fn load(home: &Path) -> Result<Agents, String> {
    let path = home.join(FILE);
    match fs::read(&path) {
        Ok(bytes) => {
            let agents: Agents =
                serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            if agents.schema != 1 {
                return Err(format!("{}: unsupported agents file", path.display()));
            }
            Ok(agents)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Agents {
            schema: 1,
            agents: vec![],
        }),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}
/// Every registered agent, in registration order.
pub fn list(home: &Path) -> Result<Vec<Agent>, String> {
    load(home).map(|a| a.agents)
}
/// The registered agent names.
pub fn registered(home: &Path) -> Result<Vec<String>, String> {
    list(home).map(|a| a.into_iter().map(|a| a.name).collect())
}
/// Registers `name`; returns false when it was already registered.
pub fn register(home: &Path, name: &str) -> Result<bool, String> {
    let name = valid(name)?;
    crate::paths::locked(home, LOCK, || {
        let mut agents = load(home)?;
        if agents.agents.iter().any(|a| a.name == name) {
            return Ok(false);
        }
        agents.agents.push(Agent {
            name,
            registered_unix: crate::research::unix_now(),
        });
        let bytes = serde_json::to_vec_pretty(&agents).map_err(|e| e.to_string())?;
        crate::research::atomic(&home.join(FILE), &bytes)?;
        Ok(true)
    })
}
