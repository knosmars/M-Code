//! Language detection + server metadata, and the lang->client map.

use super::client::LspClient;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Lang {
    Rust,
    TypeScript,
}

impl Lang {
    /// LSP `languageId` for `didOpen`.
    pub fn language_id(self) -> &'static str {
        match self {
            Lang::Rust => "rust",
            Lang::TypeScript => "typescript",
        }
    }

    /// Server binary + args.
    pub fn command(self) -> (&'static str, Vec<&'static str>) {
        match self {
            Lang::Rust => ("rust-analyzer", vec![]),
            Lang::TypeScript => ("typescript-language-server", vec!["--stdio"]),
        }
    }

    /// Per-request readiness budget (rust-analyzer indexes slowly).
    pub fn timeout(self) -> Duration {
        match self {
            Lang::Rust => Duration::from_secs(30),
            Lang::TypeScript => Duration::from_secs(5),
        }
    }
}

/// Map a file extension to a supported language, or `None` (→ regex fallback).
pub fn lang_for_ext(ext: &str) -> Option<Lang> {
    match ext {
        "rs" => Some(Lang::Rust),
        "ts" | "tsx" | "js" | "jsx" | "mts" | "cts" | "mjs" | "cjs" => Some(Lang::TypeScript),
        _ => None,
    }
}

/// A language slot: a live client, or a recorded "binary missing / spawn
/// failed" so we don't retry spawning on every call (callers fall back).
enum Slot {
    Ready(Arc<Mutex<LspClient>>),
    Unavailable,
}

fn registry() -> &'static Mutex<HashMap<Lang, Slot>> {
    static REGISTRY: OnceLock<Mutex<HashMap<Lang, Slot>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Get the client for `lang`, spawning it once if needed. Returns `None` when
/// the server is unavailable (binary missing / spawn failed) → caller falls
/// back to the regex impl. The registry mutex is held only across lookup/spawn;
/// the (slow) query then takes the per-client lock, so one language's indexing
/// never blocks another's queries.
pub fn get_or_spawn(lang: Lang, root_uri: &str) -> Option<Arc<Mutex<LspClient>>> {
    let mut reg = registry().lock().ok()?;
    match reg.get(&lang) {
        Some(Slot::Ready(c)) => return Some(c.clone()),
        Some(Slot::Unavailable) => return None,
        None => {}
    }
    match LspClient::spawn(lang, root_uri) {
        Ok(c) => {
            let arc = Arc::new(Mutex::new(c));
            reg.insert(lang, Slot::Ready(arc.clone()));
            Some(arc)
        }
        Err(e) => {
            log::warn!("[lsp] spawn {lang:?} failed: {e}");
            reg.insert(lang, Slot::Unavailable);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_ext() {
        assert_eq!(lang_for_ext("rs"), Some(Lang::Rust));
    }

    #[test]
    fn typescript_family() {
        for e in ["ts", "tsx", "js", "jsx", "mts", "cts", "mjs", "cjs"] {
            assert_eq!(lang_for_ext(e), Some(Lang::TypeScript), "ext {e}");
        }
    }

    #[test]
    fn unsupported_is_none() {
        assert_eq!(lang_for_ext("py"), None);
        assert_eq!(lang_for_ext("go"), None);
        assert_eq!(lang_for_ext(""), None);
    }

    #[test]
    fn metadata() {
        assert_eq!(Lang::Rust.language_id(), "rust");
        assert_eq!(Lang::TypeScript.command().1, vec!["--stdio"]);
    }
}
