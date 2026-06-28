use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TriggerDef {
    #[serde(rename = "file_watch")]
    FileWatch {
        id: String,
        #[serde(default)]
        glob: String,
        #[serde(default)]
        command: String,
        #[serde(default)]
        auto_run: bool,
    },
    #[serde(rename = "schedule")]
    Schedule {
        id: String,
        cron: String,
        #[serde(default)]
        command: String,
        #[serde(default)]
        auto_run: bool,
    },
    #[serde(rename = "webhook")]
    Webhook {
        id: String,
        #[serde(default)]
        port: u16,
        #[serde(default)]
        command: String,
        #[serde(default)]
        auto_run: bool,
    },
}

impl TriggerDef {
    pub fn id(&self) -> &str {
        match self {
            TriggerDef::FileWatch { id, .. }
            | TriggerDef::Schedule { id, .. }
            | TriggerDef::Webhook { id, .. } => id,
        }
    }
    pub fn auto_run(&self) -> bool {
        match self {
            TriggerDef::FileWatch { auto_run, .. }
            | TriggerDef::Schedule { auto_run, .. }
            | TriggerDef::Webhook { auto_run, .. } => *auto_run,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggersFile {
    #[serde(default)]
    pub triggers: Vec<TriggerDef>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TriggersOutput {
    pub triggers: Vec<TriggerDef>,
    pub active_count: usize,
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Read .meyatu/triggers.yml and return parsed trigger definitions.
#[tauri::command]
pub fn tool_triggers_list(path: String) -> Result<TriggersOutput, String> {
    let root = super::resolve_workspace_path(&path)?;
    let triggers_path = root.join(".meyatu").join("triggers.yml");

    let file: TriggersFile = if triggers_path.exists() {
        let raw = fs::read_to_string(&triggers_path).map_err(|e| e.to_string())?;
        serde_yaml::from_str(&raw).unwrap_or(TriggersFile { triggers: vec![] })
    } else {
        TriggersFile { triggers: vec![] }
    };

    let active_count = file.triggers.iter().filter(|t| t.auto_run()).count();

    Ok(TriggersOutput {
        triggers: file.triggers,
        active_count,
    })
}

/// Start a single trigger by id in a background thread.
/// Returns immediately; the trigger runs until the Tauri app exits.
#[tauri::command]
pub fn tool_triggers_watch(path: String, trigger_id: String) -> Result<String, String> {
    let root = super::resolve_workspace_path(&path)?;
    let file = read_triggers(&root)?;

    let def = file
        .triggers
        .iter()
        .find(|t| t.id() == trigger_id)
        .cloned()
        .ok_or_else(|| format!("Trigger not found: {trigger_id}"))?;

    start_trigger(&root, &def)?;
    Ok(format!("Trigger {trigger_id} started"))
}

/// Start every trigger whose `auto_run` flag is set. Intended to be invoked once
/// a workspace is selected so background watchers come up without the model
/// having to call `triggers_watch` for each one. Triggers that fail to start
/// (e.g. a webhook port already in use) are skipped and reported, so one bad
/// trigger never blocks the rest.
#[tauri::command]
pub fn tool_triggers_start_auto(path: String) -> Result<String, String> {
    let root = super::resolve_workspace_path(&path)?;
    let file = read_triggers(&root)?;

    let mut started: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    for def in file.triggers.iter().filter(|t| t.auto_run()) {
        match start_trigger(&root, def) {
            Ok(()) => started.push(def.id().to_string()),
            Err(e) => failed.push(format!("{}: {e}", def.id())),
        }
    }

    let mut msg = format!("Started {} auto_run trigger(s)", started.len());
    if !started.is_empty() {
        msg.push_str(&format!(" [{}]", started.join(", ")));
    }
    if !failed.is_empty() {
        msg.push_str(&format!("; {} failed: {}", failed.len(), failed.join("; ")));
    }
    Ok(msg)
}

fn read_triggers(root: &Path) -> Result<TriggersFile, String> {
    let triggers_path = root.join(".meyatu").join("triggers.yml");
    if !triggers_path.exists() {
        return Ok(TriggersFile { triggers: vec![] });
    }
    let raw = fs::read_to_string(&triggers_path).map_err(|e| e.to_string())?;
    serde_yaml::from_str(&raw).map_err(|e| format!("Failed to parse triggers.yml: {e}"))
}

/// Spawn the background loop/listener for a single trigger definition.
/// The spawned thread runs for the lifetime of the process.
fn start_trigger(root: &Path, def: &TriggerDef) -> Result<(), String> {
    match def {
        TriggerDef::FileWatch { glob, command, .. } => {
            if glob.is_empty() || command.is_empty() {
                return Err("file_watch trigger requires glob and command".into());
            }
            let glob = glob.clone();
            let cmd = command.clone();
            let root_path = root.to_path_buf();

            thread::spawn(move || {
                // Simple polling-based file watcher
                let mut last_modified: std::collections::HashMap<String, u64> =
                    std::collections::HashMap::new();
                loop {
                    scan_and_fire(&glob, &root_path, &mut last_modified, &cmd);
                    thread::sleep(Duration::from_secs(2));
                }
            });
            Ok(())
        }
        TriggerDef::Schedule { cron, command, .. } => {
            if command.is_empty() {
                return Err("schedule trigger requires command".into());
            }
            let parts: Vec<&str> = cron.split_whitespace().collect();
            if parts.len() != 5 {
                return Err(format!(
                    "schedule trigger cron must have 5 fields, got {}",
                    parts.len()
                ));
            }
            let interval_seconds = parse_cron_interval(&parts).unwrap_or(3600);
            let cmd = command.clone();

            thread::spawn(move || loop {
                thread::sleep(Duration::from_secs(interval_seconds));
                let _ = shell_command(&cmd).output();
            });
            Ok(())
        }
        TriggerDef::Webhook { port, command, .. } => {
            if command.is_empty() {
                return Err("webhook trigger requires command".into());
            }
            if *port == 0 {
                return Err("webhook trigger requires a non-zero port".into());
            }
            // Bind synchronously so a port conflict is reported to the caller
            // instead of silently dying inside the spawned thread.
            let addr = format!("127.0.0.1:{port}");
            let listener = TcpListener::bind(&addr)
                .map_err(|e| format!("failed to bind webhook port {port}: {e}"))?;
            let cmd = command.clone();
            let root_path = root.to_path_buf();

            thread::spawn(move || {
                for stream in listener.incoming() {
                    match stream {
                        Ok(conn) => handle_webhook_conn(conn, &cmd, &root_path),
                        Err(_) => continue,
                    }
                }
            });
            Ok(())
        }
    }
}

/// Read an incoming HTTP request, fire the configured command on POST, and
/// reply with a minimal HTTP response. GET requests get a 200 health-check;
/// anything that isn't POST does not fire the command.
fn handle_webhook_conn(mut conn: std::net::TcpStream, command: &str, root: &Path) {
    let mut buf = [0u8; 1024];
    let method = match conn.read(&mut buf) {
        Ok(n) if n > 0 => {
            let head = String::from_utf8_lossy(&buf[..n]);
            head.split_whitespace().next().unwrap_or("").to_uppercase()
        }
        _ => String::new(),
    };

    let (status, fired) = if method == "POST" {
        let _ = shell_command(command).current_dir(root).output();
        ("200 OK", true)
    } else if method == "GET" {
        ("200 OK", false)
    } else {
        ("405 Method Not Allowed", false)
    };

    let body = if fired {
        "{\"status\":\"fired\"}"
    } else {
        "{\"status\":\"ok\"}"
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = conn.write_all(response.as_bytes());
    let _ = conn.flush();
}

fn scan_and_fire(
    glob: &str,
    root_path: &Path,
    last_modified: &mut std::collections::HashMap<String, u64>,
    command: &str,
) {
    // Convert simple glob to walkdir pattern
    let pattern = glob.replace("**/*", "");
    let extension = pattern.trim_start_matches('*').trim_start_matches('.');
    let target_dir = if glob.starts_with("**/") {
        root_path.to_path_buf()
    } else if let Some(idx) = glob.find('/') {
        root_path.join(&glob[..idx])
    } else {
        root_path.to_path_buf()
    };

    let walker = walkdir::WalkDir::new(&target_dir)
        .max_depth(10)
        .into_iter()
        .filter_map(|e| e.ok());

    let mut changed = false;
    for entry in walker {
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .is_some_and(|e| e == extension || extension.is_empty())
        {
            if let Ok(meta) = path.metadata() {
                if let Ok(mod_time) = meta.modified() {
                    let secs = mod_time
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let key = path.to_string_lossy().to_string();
                    let prev = last_modified.insert(key, secs);
                    if prev != Some(secs) {
                        changed = true;
                        break;
                    }
                }
            }
        }
    }

    if changed {
        let _ = shell_command(command).output();
    }
}

fn shell_command(command: &str) -> std::process::Command {
    if cfg!(target_os = "windows") {
        let mut cmd = std::process::Command::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    } else {
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg(command);
        cmd
    }
}

fn parse_cron_interval(parts: &[&str]) -> Option<u64> {
    // Parse minute field: "*/N" means every N minutes
    let minute = parts[0];
    if let Some(remaining) = minute.strip_prefix("*/") {
        return remaining.parse::<u64>().ok().map(|n| n * 60);
    }
    // "0 9 * * *" style: non-wildcard minute → at least hourly.
    // Wildcard-only (all "*") → every minute (60s).
    // Any non-wildcard field means a specific time of day → hourly polling.
    let has_specific = parts.iter().any(|p| *p != "*");
    if has_specific {
        return Some(3600); // poll hourly to check if it's time
    }
    Some(60)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_triggers() {
        let yaml = "triggers: []";
        let file: TriggersFile = serde_yaml::from_str(yaml).unwrap();
        assert!(file.triggers.is_empty());
    }

    #[test]
    fn parse_file_watch_trigger() {
        let yaml = r#"
triggers:
  - type: file_watch
    id: watch-rust
    glob: "src/**/*.rs"
    command: "cargo clippy"
    auto_run: true
"#;
        let file: TriggersFile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(file.triggers.len(), 1);
        assert_eq!(file.triggers[0].id(), "watch-rust");
        assert!(file.triggers[0].auto_run());
    }

    #[test]
    fn parse_schedule_trigger() {
        let yaml = r#"
triggers:
  - type: schedule
    id: daily-lint
    cron: "0 9 * * *"
    command: "cargo clippy"
"#;
        let file: TriggersFile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(file.triggers.len(), 1);
        assert_eq!(file.triggers[0].id(), "daily-lint");
    }

    #[test]
    fn parse_webhook_trigger() {
        let yaml = r#"
triggers:
  - type: webhook
    id: gh-push
    port: 9000
    command: "echo webhook"
"#;
        let file: TriggersFile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(file.triggers.len(), 1);
        assert_eq!(file.triggers[0].id(), "gh-push");
    }

    #[test]
    fn parse_all_trigger_types() {
        let yaml = r#"
triggers:
  - type: file_watch
    id: w1
    glob: "*.rs"
    command: "cargo check"
  - type: schedule
    id: s1
    cron: "* * * * *"
    command: "echo"
  - type: webhook
    id: wh1
    port: 8080
    command: "echo"
"#;
        let file: TriggersFile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(file.triggers.len(), 3);
    }

    #[test]
    fn webhook_fires_command_on_post() {
        use std::io::{Read, Write};
        use std::net::TcpStream;

        // A unique marker file the command will create when fired.
        let marker = std::env::temp_dir().join(format!("meyatu_webhook_{}.flag", std::process::id()));
        let _ = fs::remove_file(&marker);
        let cmd = format!("touch {}", marker.to_string_lossy());

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let root = std::env::temp_dir();
        thread::spawn(move || {
            if let Ok((conn, _)) = listener.accept() {
                handle_webhook_conn(conn, &cmd, &root);
            }
        });

        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .write_all(b"POST /hook HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        let mut resp = String::new();
        stream.read_to_string(&mut resp).unwrap();
        assert!(resp.contains("200 OK"), "response was: {resp}");
        assert!(resp.contains("fired"), "response was: {resp}");

        // Give the spawned command a moment to create the marker.
        for _ in 0..50 {
            if marker.exists() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(marker.exists(), "command should have created marker file");
        let _ = fs::remove_file(&marker);
    }

    #[test]
    fn webhook_get_does_not_fire() {
        use std::io::{Read, Write};
        use std::net::TcpStream;

        let marker =
            std::env::temp_dir().join(format!("meyatu_webhook_get_{}.flag", std::process::id()));
        let _ = fs::remove_file(&marker);
        let cmd = format!("touch {}", marker.to_string_lossy());

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let root = std::env::temp_dir();
        thread::spawn(move || {
            if let Ok((conn, _)) = listener.accept() {
                handle_webhook_conn(conn, &cmd, &root);
            }
        });

        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .write_all(b"GET /hook HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        let mut resp = String::new();
        stream.read_to_string(&mut resp).unwrap();
        assert!(resp.contains("200 OK"), "response was: {resp}");
        thread::sleep(Duration::from_millis(100));
        assert!(!marker.exists(), "GET must not fire the command");
        let _ = fs::remove_file(&marker);
    }

    #[test]
    fn start_trigger_webhook_rejects_zero_port() {
        let def = TriggerDef::Webhook {
            id: "wh".into(),
            port: 0,
            command: "echo hi".into(),
            auto_run: true,
        };
        let err = start_trigger(&std::env::temp_dir(), &def).unwrap_err();
        assert!(err.contains("non-zero port"), "got: {err}");
    }

    #[test]
    fn start_trigger_webhook_requires_command() {
        let def = TriggerDef::Webhook {
            id: "wh".into(),
            port: 9999,
            command: String::new(),
            auto_run: true,
        };
        let err = start_trigger(&std::env::temp_dir(), &def).unwrap_err();
        assert!(err.contains("requires a command") || err.contains("requires command"), "got: {err}");
    }

    #[test]
    fn cron_interval_parse() {
        assert_eq!(parse_cron_interval(&["*/", "5", "*", "*", "*"]), None);
        assert_eq!(parse_cron_interval(&["*/5", "*", "*", "*", "*"]), Some(300));
        assert_eq!(parse_cron_interval(&["*", "*", "*", "*", "*"]), Some(60));
        // Non-wildcard minute fields → hourly polling (specific time-of-day crons)
        assert_eq!(parse_cron_interval(&["0", "9", "*", "*", "*"]), Some(3600));
        assert_eq!(parse_cron_interval(&["30", "14", "*", "*", "1-5"]), Some(3600));
    }
}
