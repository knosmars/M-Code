use std::path::PathBuf;

fn main() {
    let (client_id, client_secret) = resolve_credentials();

    println!("cargo:rustc-env=GITHUB_OAUTH_CLIENT_ID={}", client_id);
    println!("cargo:rustc-env=GITHUB_OAUTH_CLIENT_SECRET={}", client_secret);
    println!("cargo:rerun-if-env-changed=GITHUB_OAUTH_CLIENT_ID");
    println!("cargo:rerun-if-env-changed=GITHUB_OAUTH_CLIENT_SECRET");

    tauri_build::build()
}

fn resolve_credentials() -> (String, String) {
    let env_id = std::env::var("GITHUB_OAUTH_CLIENT_ID").unwrap_or_default();
    let env_secret = std::env::var("GITHUB_OAUTH_CLIENT_SECRET").unwrap_or_default();
    if !env_id.is_empty() && !env_secret.is_empty() {
        return (env_id, env_secret);
    }

    let path = creds_file_path();
    if let Ok(content) = std::fs::read_to_string(&path) {
        let (fid, fsecret) = parse_creds_file(&content);
        if !fid.is_empty() && !fsecret.is_empty() {
            println!("cargo:rerun-if-changed={}", path.display());
            return (fid, fsecret);
        }
    }

    (env_id, env_secret)
}

fn creds_file_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    PathBuf::from(home).join(".github-oauth")
}

fn parse_creds_file(content: &str) -> (String, String) {
    let mut client_id = String::new();
    let mut client_secret = String::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            match key.trim() {
                "GITHUB_OAUTH_CLIENT_ID" | "CLIENT_ID" => {
                    client_id = value.trim().to_string();
                }
                "GITHUB_OAUTH_CLIENT_SECRET" | "CLIENT_SECRET" => {
                    client_secret = value.trim().to_string();
                }
                _ => {}
            }
        }
    }

    (client_id, client_secret)
}
