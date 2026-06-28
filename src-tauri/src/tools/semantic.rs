//! Semantic code search via Ollama embeddings + brute-force cosine.
//!
//! `tool_semantic_index` embeds the workspace into `.meyatu/vectors.db`;
//! `tool_semantic_search` embeds a query and returns the most similar chunks.
//! Coexists with the keyword `search_codebase` tool.

use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const DEFAULT_EMBED_BASE: &str = "http://localhost:11434";
const DEFAULT_EMBED_MODEL: &str = "nomic-embed-text";

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct SemanticConfig {
    embed_base: String,
    embed_model: String,
}

impl Default for SemanticConfig {
    fn default() -> Self {
        Self {
            embed_base: DEFAULT_EMBED_BASE.to_string(),
            embed_model: DEFAULT_EMBED_MODEL.to_string(),
        }
    }
}

fn config_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok()?;
    Some(PathBuf::from(home).join(".config/meyatu-code/semantic.json"))
}

fn read_config() -> SemanticConfig {
    config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

#[cfg(test)]
fn read_config_at(path: &Path) -> SemanticConfig {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_config_at(path: &Path, cfg: &SemanticConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}
const CHUNK_WINDOW: usize = 40;
const CHUNK_OVERLAP: usize = 5;
const CHUNK_TARGET: usize = 40;
const CHUNK_MAX: usize = 100;
const CHUNK_VERSION: &str = "syntax-v1";
const BRACE_EXTS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "go", "java",
    "c", "h", "cpp", "hpp", "cc", "cs",
];
const EMBED_BATCH: usize = 32;
const MAX_FILE_BYTES: u64 = 256 * 1024;
const IGNORE_DIRS: &[&str] = &["node_modules", "target", ".git", "dist", "build", ".meyatu", ".next", "vendor"];

// ── Pure helpers (unit-tested) ──────────────────────────────────────────

/// Cosine similarity; 0.0 for mismatched/empty/zero vectors.
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// Split into fixed line windows. Returns `(start_line_1based, end_line, text)`.
fn chunk_lines(content: &str, window: usize, overlap: usize) -> Vec<(usize, usize, String)> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }
    let step = window.saturating_sub(overlap).max(1);
    let mut out = Vec::new();
    let mut start = 0;
    while start < lines.len() {
        let end = (start + window).min(lines.len());
        out.push((start + 1, end, lines[start..end].join("\n")));
        if end == lines.len() {
            break;
        }
        start += step;
    }
    out
}

#[derive(Clone, Copy, PartialEq)]
enum ScanState {
    Normal,
    BlockComment,
    Template,
}

/// 字符字面量判定：`bytes[i] == '\''`，匹配 `'\?.'` 形（一字符、可选转义、再 `'`）。
/// 正确处理 `'{'`/`'\n'`，不误判 Rust 生命周期 `'a`（不闭合 → false）。
fn is_char_literal(bytes: &[char], i: usize) -> bool {
    if bytes[i + 1..].is_empty() {
        return false;
    }
    if bytes[i + 1] == '\\' {
        i + 3 < bytes.len() && bytes[i + 3] == '\''
    } else {
        i + 2 < bytes.len() && bytes[i + 2] == '\''
    }
}

/// 扫一行，更新 brace `depth` 与跨行状态（块注释 / 模板串）。返回行尾状态。
/// 串/注释/字符字面量内的花括号不计入 depth。
fn scan_line(line: &str, depth: &mut i32, state: ScanState) -> ScanState {
    let bytes: Vec<char> = line.chars().collect();
    let mut i = 0;
    let mut st = state;
    while i < bytes.len() {
        let c = bytes[i];
        match st {
            ScanState::BlockComment => {
                if c == '*' && bytes.get(i + 1) == Some(&'/') {
                    st = ScanState::Normal;
                    i += 2;
                    continue;
                }
                i += 1;
            }
            ScanState::Template => {
                if c == '\\' {
                    i += 2;
                    continue;
                }
                if c == '`' {
                    st = ScanState::Normal;
                }
                i += 1;
            }
            ScanState::Normal => {
                if c == '/' && bytes.get(i + 1) == Some(&'/') {
                    break; // 行注释，余下忽略
                }
                if c == '/' && bytes.get(i + 1) == Some(&'*') {
                    st = ScanState::BlockComment;
                    i += 2;
                    continue;
                }
                if c == '"' {
                    i += 1;
                    while i < bytes.len() {
                        if bytes[i] == '\\' {
                            i += 2;
                            continue;
                        }
                        if bytes[i] == '"' {
                            i += 1;
                            break;
                        }
                        i += 1;
                    }
                    continue;
                }
                if c == '`' {
                    st = ScanState::Template;
                    i += 1;
                    continue;
                }
                if c == '\'' {
                    if is_char_literal(&bytes, i) {
                        let mut j = i + 1;
                        if bytes[j] == '\\' {
                            j += 1;
                        }
                        j += 1; // 字符本身
                        if bytes.get(j) == Some(&'\'') {
                            j += 1;
                        }
                        i = j;
                        continue;
                    }
                    // 生命周期 / 散落引号：当普通字符
                    i += 1;
                    continue;
                }
                if c == '{' {
                    *depth += 1;
                } else if c == '}' {
                    *depth -= 1;
                }
                i += 1;
            }
        }
    }
    st
}

/// 花括号系源码语法感知分块：顶层 (depth 0) item 软打包至 `target` 行才切；
/// 单段超 `max` 行 → 回退 `chunk_lines(target, overlap)` 子切。1-based `(start, end, text)`。
fn chunk_syntax(content: &str, target: usize, max: usize, overlap: usize) -> Vec<(usize, usize, String)> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }
    // Pass 1：软打包段 [start_idx0, end_idx_excl)
    let mut segments: Vec<(usize, usize)> = Vec::new();
    let mut depth: i32 = 0;
    let mut state = ScanState::Normal;
    let mut seg_start = 0usize;
    for (i, line) in lines.iter().enumerate() {
        state = scan_line(line, &mut depth, state);
        let at_boundary = depth <= 0 && state == ScanState::Normal;
        let accumulated = i + 1 - seg_start;
        if at_boundary && accumulated >= target {
            segments.push((seg_start, i + 1));
            seg_start = i + 1;
            depth = 0; // 防御不平衡 } 的负漂移
        }
    }
    if seg_start < lines.len() {
        segments.push((seg_start, lines.len()));
    }
    // Pass 2：展开超长段为行窗
    let mut out: Vec<(usize, usize, String)> = Vec::new();
    for (s, e) in segments {
        if e - s > max {
            let slice = lines[s..e].join("\n");
            for (ss, ee, text) in chunk_lines(&slice, target, overlap) {
                out.push((s + ss, s + ee, text)); // 子切 1-based → 绝对行号
            }
        } else {
            out.push((s + 1, e, lines[s..e].join("\n")));
        }
    }
    out
}

fn vec_to_blob(v: &[f32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(v.len() * 4);
    for x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b
}

fn blob_to_vec(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Indices of the `k` highest scores, descending.
fn top_k_indices(scores: &[f32], k: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..scores.len()).collect();
    idx.sort_by(|&a, &b| scores[b].partial_cmp(&scores[a]).unwrap_or(std::cmp::Ordering::Equal));
    idx.truncate(k);
    idx
}

// ── Embedding (Ollama, OpenAI-compatible) ───────────────────────────────

async fn embed(inputs: Vec<String>, base: &str, model: &str) -> Result<Vec<Vec<f32>>, String> {
    let resp = reqwest::Client::new()
        .post(format!("{}/v1/embeddings", crate::provider::normalize_api_base(base)))
        .json(&json!({ "model": model, "input": inputs }))
        .send()
        .await
        .map_err(|e| format!("embedding 请求失败（端点 {base} 是否在运行且模型 `{model}` 可用？）: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("embedding 端点返回 {}", resp.status()));
    }
    let data: Value = resp.json().await.map_err(|e| format!("解析 embedding 响应失败: {e}"))?;
    let arr = data.get("data").and_then(|d| d.as_array()).ok_or("embedding 响应缺少 data")?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let emb = item.get("embedding").and_then(|e| e.as_array()).ok_or("embedding 条目缺少 embedding")?;
        out.push(emb.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect());
    }
    Ok(out)
}

/// Embed all inputs, batched to limit round-trips.
async fn embed_all(inputs: Vec<String>, base: &str, model: &str) -> Result<Vec<Vec<f32>>, String> {
    let mut out = Vec::with_capacity(inputs.len());
    for batch in inputs.chunks(EMBED_BATCH) {
        out.extend(embed(batch.to_vec(), base, model).await?);
    }
    Ok(out)
}

// ── Storage ─────────────────────────────────────────────────────────────

fn db_path(workspace: &str) -> PathBuf {
    Path::new(workspace).join(".meyatu").join("vectors.db")
}

fn open_db(workspace: &str) -> Result<Connection, String> {
    let p = db_path(workspace);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("无法创建 .meyatu 目录: {e}"))?;
    }
    let conn = Connection::open(&p).map_err(|e| format!("无法打开向量库: {e}"))?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS chunks (path TEXT, start_line INTEGER, end_line INTEGER, content TEXT, vector BLOB, mtime INTEGER)",
        [],
    )
    .map_err(|e| e.to_string())?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_chunks_path ON chunks(path)", [])
        .map_err(|e| e.to_string())?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT)",
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(conn)
}

fn meta_get(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
        .ok()
}

fn meta_set(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 索引代数：chunk 表每次变更递增，作为检索缓存失效信号。缺省 0。
fn read_generation(conn: &Connection) -> i64 {
    meta_get(conn, "index_generation")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0)
}

fn bump_generation(conn: &Connection) -> Result<(), String> {
    let next = read_generation(conn) + 1;
    meta_set(conn, "index_generation", &next.to_string())
}

/// 库里记录的模型与当前不同 → 需全量重建；无记录（首次）→ false。
fn needs_full_rebuild(stored_model: Option<&str>, current: &str) -> bool {
    matches!(stored_model, Some(m) if m != current)
}

/// 库记录的分块版本与当前不同（含旧库无记录 None）→ 需全量重建。
fn cv_needs_rebuild(stored: Option<&str>, current: &str) -> bool {
    stored != Some(current)
}

/// 模型或分块版本变更则清空 chunks（迫使全量重嵌）并失效 embed_dim，记录当前 model + chunk_version。
fn ensure_index_meta(workspace: &str, model: &str, chunk_version: &str) -> Result<(), String> {
    let conn = open_db(workspace)?;
    let stored_model = meta_get(&conn, "embed_model");
    let stored_cv = meta_get(&conn, "chunk_version");
    if needs_full_rebuild(stored_model.as_deref(), model)
        || cv_needs_rebuild(stored_cv.as_deref(), chunk_version)
    {
        conn.execute("DELETE FROM chunks", []).map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM meta WHERE key = 'embed_dim'", [])
            .map_err(|e| e.to_string())?;
        bump_generation(&conn)?;
    }
    meta_set(&conn, "embed_model", model)?;
    meta_set(&conn, "chunk_version", chunk_version)?;
    Ok(())
}

#[allow(dead_code)]
fn read_embed_dim(workspace: &str) -> Option<usize> {
    let conn = open_db(workspace).ok()?;
    meta_get(&conn, "embed_dim").and_then(|s| s.parse::<usize>().ok())
}

/// 已记录维度且与 query 维度不同 → 不符。无记录 → false（向后兼容）。
fn dim_mismatch(query_dim: usize, stored_dim: Option<usize>) -> bool {
    matches!(stored_dim, Some(d) if d != query_dim)
}

/// 库中存在但工作区已不存在的 path。
fn orphan_paths(stored: &[String], live: &HashSet<String>) -> Vec<String> {
    stored.iter().filter(|p| !live.contains(*p)).cloned().collect()
}

/// 删除 live 集外（已删除文件）的 chunk，返回删除的文件数。
fn reconcile_deletions(workspace: &str, live: &HashSet<String>) -> Result<usize, String> {
    let conn = open_db(workspace)?;
    let stored: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT DISTINCT path FROM chunks")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        let mut v = Vec::new();
        for row in rows {
            v.push(row.map_err(|e| e.to_string())?);
        }
        v
    };
    let orphans = orphan_paths(&stored, live);
    for p in &orphans {
        conn.execute("DELETE FROM chunks WHERE path = ?1", [p])
            .map_err(|e| e.to_string())?;
    }
    if !orphans.is_empty() {
        bump_generation(&conn)?;
    }
    Ok(orphans.len())
}

struct FileChunks {
    path: String,
    mtime: i64,
    chunks: Vec<(usize, usize, String)>,
}

/// 花括号系扩展名 → 走 chunk_syntax；否则回退 chunk_lines。
fn is_brace_ext(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| BRACE_EXTS.contains(&e))
        .unwrap_or(false)
}

/// Walk the workspace and collect chunks for files that are new or changed
/// (by mtime). Returns `(files_to_index, skipped_count, live_paths)`.
fn build_index_plan(workspace: &str) -> Result<(Vec<FileChunks>, usize, HashSet<String>), String> {
    let conn = open_db(workspace)?;
    let mut files = Vec::new();
    let mut skipped = 0usize;
    let mut live: HashSet<String> = HashSet::new();
    for entry in WalkDir::new(workspace)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !IGNORE_DIRS.contains(&name.as_ref()) && !name.starts_with('.')
                || e.depth() == 0
        })
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) if m.len() <= MAX_FILE_BYTES => m,
            _ => continue,
        };
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let path = entry.path().to_string_lossy().to_string();
        live.insert(path.clone());
        let stored: Option<i64> = conn
            .query_row("SELECT mtime FROM chunks WHERE path = ?1 LIMIT 1", [&path], |r| r.get(0))
            .ok();
        if stored == Some(mtime) {
            skipped += 1;
            continue;
        }
        let content = match std::fs::read_to_string(entry.path()) {
            Ok(c) => c,
            Err(_) => continue, // binary / unreadable
        };
        let chunks = if is_brace_ext(&path) {
            chunk_syntax(&content, CHUNK_TARGET, CHUNK_MAX, CHUNK_OVERLAP)
        } else {
            chunk_lines(&content, CHUNK_WINDOW, CHUNK_OVERLAP)
        };
        if !chunks.is_empty() {
            files.push(FileChunks { path, mtime, chunks });
        }
    }
    Ok((files, skipped, live))
}

fn write_index(workspace: &str, files: Vec<FileChunks>, vectors: Vec<Vec<f32>>) -> Result<usize, String> {
    let mut conn = open_db(workspace)?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let mut vi = 0usize;
    let mut written = 0usize;
    for f in &files {
        tx.execute("DELETE FROM chunks WHERE path = ?1", [&f.path]).map_err(|e| e.to_string())?;
        for (start, end, content) in &f.chunks {
            let vector = vectors.get(vi).map(|v| vec_to_blob(v)).unwrap_or_default();
            vi += 1;
            tx.execute(
                "INSERT INTO chunks (path, start_line, end_line, content, vector, mtime) VALUES (?1,?2,?3,?4,?5,?6)",
                rusqlite::params![f.path, *start as i64, *end as i64, content, vector, f.mtime],
            )
            .map_err(|e| e.to_string())?;
            written += 1;
        }
    }
    if let Some(dim) = vectors.iter().find(|v| !v.is_empty()).map(|v| v.len()) {
        tx.execute(
            "INSERT INTO meta (key, value) VALUES ('embed_dim', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [dim.to_string()],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    bump_generation(&conn)?;
    Ok(written)
}

struct StoredChunk {
    path: String,
    start_line: i64,
    end_line: i64,
    content: String,
    vector: Vec<f32>,
}

fn load_chunks(workspace: &str) -> Result<Vec<StoredChunk>, String> {
    let conn = open_db(workspace)?;
    let mut stmt = conn
        .prepare("SELECT path, start_line, end_line, content, vector FROM chunks")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            let blob: Vec<u8> = r.get(4)?;
            Ok(StoredChunk {
                path: r.get(0)?,
                start_line: r.get(1)?,
                end_line: r.get(2)?,
                content: r.get(3)?,
                vector: blob_to_vec(&blob),
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

// ── 检索向量内存缓存 ────────────────────────────────────────────────────

/// 进程级单条向量缓存：避免每查询全量反序列化。按 workspace + index_generation 命中。
struct VectorCache {
    workspace: String,
    generation: i64,
    chunks: Vec<StoredChunk>,
}

static SEARCH_CACHE: std::sync::Mutex<Option<VectorCache>> = std::sync::Mutex::new(None);

/// 缓存命中：存在且 workspace 与 generation 均一致。
fn cache_reusable(cache: &Option<VectorCache>, workspace: &str, generation: i64) -> bool {
    matches!(cache, Some(c) if c.workspace == workspace && c.generation == generation)
}

/// 在缓存（必要时重载）上打分并返回 top-k 结果 JSON。锁在缓存重载（miss 时）及
/// 打分期持有；embedding 网络调用在锁外（tool_semantic_search 内、spawn_blocking 前）。
fn search_cached(workspace: &str, qvec: &[f32], k: usize) -> Result<Vec<Value>, String> {
    let conn = open_db(workspace)?;
    let generation = read_generation(&conn);
    let stored_dim = meta_get(&conn, "embed_dim").and_then(|s| s.parse::<usize>().ok());

    let mut guard = SEARCH_CACHE.lock().unwrap_or_else(|p| p.into_inner());
    if !cache_reusable(&guard, workspace, generation) {
        let chunks = load_chunks(workspace)?;
        *guard = Some(VectorCache {
            workspace: workspace.to_string(),
            generation,
            chunks,
        });
    }
    let cache = guard.as_ref().expect("cache just populated");

    if cache.chunks.is_empty() {
        return Err("语义索引为空，请先运行 semantic_index。".to_string());
    }
    if dim_mismatch(qvec.len(), stored_dim) {
        return Err("embedding 维度不符（模型可能已更换），请重新运行 semantic_index".to_string());
    }
    let scores: Vec<f32> = cache.chunks.iter().map(|c| cosine(qvec, &c.vector)).collect();
    let results: Vec<Value> = top_k_indices(&scores, k)
        .into_iter()
        .map(|i| {
            let c = &cache.chunks[i];
            json!({
                "path": c.path,
                "startLine": c.start_line,
                "endLine": c.end_line,
                "score": (scores[i] * 1000.0).round() / 1000.0,
                "snippet": c.content,
            })
        })
        .collect();
    Ok(results)
}

// ── Tauri commands ──────────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct SemanticStatus {
    indexed: bool,
    file_count: i64,
    chunk_count: i64,
    embed_model: Option<String>,
    embed_dim: Option<i64>,
}

/// Read-only index status for the workspace's vector DB. No Ollama call;
/// returns `indexed:false` (without creating an empty DB) when none exists.
#[tauri::command]
pub fn tool_semantic_status(path: String) -> Result<String, String> {
    let p = db_path(&path);
    if !p.exists() {
        let s = SemanticStatus {
            indexed: false, file_count: 0, chunk_count: 0, embed_model: None, embed_dim: None,
        };
        return serde_json::to_string(&s).map_err(|e| e.to_string());
    }
    let conn = Connection::open(&p).map_err(|e| format!("无法打开向量库: {e}"))?;
    let chunk_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap_or(0);
    let file_count: i64 = conn
        .query_row("SELECT COUNT(DISTINCT path) FROM chunks", [], |r| r.get(0))
        .unwrap_or(0);
    let embed_model = meta_get(&conn, "embed_model");
    let embed_dim = meta_get(&conn, "embed_dim").and_then(|s| s.parse::<i64>().ok());
    let s = SemanticStatus {
        indexed: chunk_count > 0,
        file_count,
        chunk_count,
        embed_model,
        embed_dim,
    };
    serde_json::to_string(&s).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn tool_semantic_index(path: String) -> Result<String, String> {
    let cfg = read_config();
    let ws0 = path.clone();
    let model = cfg.embed_model.clone();
    tokio::task::spawn_blocking(move || ensure_index_meta(&ws0, &model, CHUNK_VERSION))
        .await
        .map_err(|e| e.to_string())??;
    let ws = path.clone();
    let (files, skipped, live) = tokio::task::spawn_blocking(move || build_index_plan(&ws))
        .await
        .map_err(|e| e.to_string())??;

    let ws_r = path.clone();
    let removed = tokio::task::spawn_blocking(move || reconcile_deletions(&ws_r, &live))
        .await
        .map_err(|e| e.to_string())??;

    if files.is_empty() {
        return Ok(format!(
            "语义索引已最新（跳过 {skipped} 个文件，清理 {removed} 个已删除文件）"
        ));
    }
    let texts: Vec<String> = files
        .iter()
        .flat_map(|f| f.chunks.iter().map(|c| c.2.clone()))
        .collect();
    let file_count = files.len();
    let vectors = embed_all(texts, &cfg.embed_base, &cfg.embed_model).await?;
    let ws2 = path.clone();
    let written = tokio::task::spawn_blocking(move || write_index(&ws2, files, vectors))
        .await
        .map_err(|e| e.to_string())??;
    Ok(format!(
        "已索引 {file_count} 个文件，{written} 个片段（跳过 {skipped} 个未变文件，清理 {removed} 个已删除文件）"
    ))
}

#[tauri::command]
pub async fn tool_semantic_search(query: String, path: String, top_k: Option<usize>) -> Result<String, String> {
    let cfg = read_config();
    let k = top_k.unwrap_or(5).clamp(1, 50);
    // 先 embed query（async 网络，锁外）。
    let qvec = embed(vec![query], &cfg.embed_base, &cfg.embed_model)
        .await?
        .into_iter()
        .next()
        .ok_or("query embedding 为空")?;
    let ws = path.clone();
    let results = tokio::task::spawn_blocking(move || search_cached(&ws, &qvec, k))
        .await
        .map_err(|e| e.to_string())??;
    serde_json::to_string(&results).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn tool_semantic_config_get() -> Result<String, String> {
    serde_json::to_string(&read_config()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn tool_semantic_config_set(embed_base: String, embed_model: String) -> Result<(), String> {
    let p = config_path().ok_or("无法定位配置目录")?;
    write_config_at(&p, &SemanticConfig { embed_base, embed_model })
}

#[cfg(test)]
mod tests {
    use super::*;

    static SEARCH_CACHE_TEST_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn cache_reusable_logic() {
        let none: Option<VectorCache> = None;
        assert!(!cache_reusable(&none, "/ws", 1));
        let c = Some(VectorCache { workspace: "/ws".to_string(), generation: 1, chunks: vec![] });
        assert!(cache_reusable(&c, "/ws", 1));
        assert!(!cache_reusable(&c, "/ws", 2));    // generation 不同
        assert!(!cache_reusable(&c, "/other", 1)); // workspace 不同
    }

    #[test]
    fn search_cached_empty_index_errs() {
        let _guard = SEARCH_CACHE_TEST_GUARD.lock().unwrap_or_else(|p| p.into_inner());
        *SEARCH_CACHE.lock().unwrap_or_else(|p| p.into_inner()) = None;
        let ws = temp_ws("cache_empty");
        open_db(&ws).unwrap(); // 建空库
        let err = search_cached(&ws, &[1.0f32, 0.0], 1).unwrap_err();
        assert!(err.contains("语义索引为空"));
    }

    #[test]
    fn search_cached_reuses_until_generation_bumps() {
        let _guard = SEARCH_CACHE_TEST_GUARD.lock().unwrap_or_else(|p| p.into_inner());
        *SEARCH_CACHE.lock().unwrap_or_else(|p| p.into_inner()) = None;
        let ws = temp_ws("cache_reuse");
        {
            let conn = open_db(&ws).unwrap();
            conn.execute(
                "INSERT INTO chunks (path,start_line,end_line,content,vector,mtime) VALUES ('a.rs',1,2,'ORIGINAL',?1,1)",
                rusqlite::params![vec_to_blob(&[1.0f32, 0.0])],
            ).unwrap();
            meta_set(&conn, "embed_dim", "2").unwrap();
        }
        let qvec = vec![1.0f32, 0.0];
        // 首查 → 载入缓存 → ORIGINAL
        let r1 = search_cached(&ws, &qvec, 1).unwrap();
        assert_eq!(r1[0]["snippet"].as_str().unwrap(), "ORIGINAL");
        // 不 bump 直接改 db 内容
        {
            let conn = open_db(&ws).unwrap();
            conn.execute("UPDATE chunks SET content='CHANGED' WHERE path='a.rs'", []).unwrap();
        }
        // 二查 → 同 generation → 缓存复用 → 仍 ORIGINAL（证缓存生效）
        let r2 = search_cached(&ws, &qvec, 1).unwrap();
        assert_eq!(r2[0]["snippet"].as_str().unwrap(), "ORIGINAL");
        // bump generation → 失效重载 → CHANGED（证失效生效）
        {
            let conn = open_db(&ws).unwrap();
            bump_generation(&conn).unwrap();
        }
        let r3 = search_cached(&ws, &qvec, 1).unwrap();
        assert_eq!(r3[0]["snippet"].as_str().unwrap(), "CHANGED");
    }

    #[test]
    fn syntax_packs_small_items() {
        // 三个各 4 行的小 fn，总 12 行 < target 40 → 打包成 1 chunk
        let src = "fn a() {\n    1\n    2\n}\nfn b() {\n    3\n    4\n}\nfn c() {\n    5\n    6\n}";
        let chunks = chunk_syntax(src, 40, 100, 5);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].0, 1);
        assert_eq!(chunks[0].1, 12);
    }

    #[test]
    fn syntax_cuts_at_depth_zero_boundary() {
        // 两个各 50 行的块 (>target) → 两 chunk，第二块 start 在第一块 close 之后
        let block = |label: char| {
            let mut s = format!("fn {label}() {{\n");
            for i in 0..48 {
                s.push_str(&format!("    let x{i} = {i};\n"));
            }
            s.push('}');
            s
        };
        let src = format!("{}\n{}", block('a'), block('b'));
        let chunks = chunk_syntax(&src, 40, 100, 5);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].0, 1);
        assert_eq!(chunks[0].1, 50);
        assert_eq!(chunks[1].0, 51); // 紧接第一块 close
    }

    #[test]
    fn syntax_subsplits_oversized_item() {
        // 单 fn 150 行 (>max 100) → 行窗子切，首 chunk start==1，连续覆盖到末行
        let mut s = String::from("fn big() {\n");
        for i in 0..148 {
            s.push_str(&format!("    let x{i} = {i};\n"));
        }
        s.push('}'); // 共 150 行
        let chunks = chunk_syntax(&s, 40, 100, 5);
        assert!(chunks.len() > 1, "oversized item must sub-split");
        assert_eq!(chunks[0].0, 1);
        assert_eq!(chunks.last().unwrap().1, 150);
    }

    #[test]
    fn syntax_ignores_braces_in_strings_and_comments() {
        // 串内 / 块注释内 / 字符字面量 的花括号不计入 depth → 不提前切，整体一段
        let src = "fn f() {\n    let s = \"}\";\n    /* { { */\n    let c = '{';\n    let d = '}';\n}";
        let chunks = chunk_syntax(src, 40, 100, 5);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].1, 6); // 6 行整体，depth 正确归零于末行
    }

    #[test]
    fn syntax_lifetime_not_treated_as_string() {
        // Rust 生命周期 'a 不是字符字面量，花括号正常计数 → 单段
        let src = "fn f<'a>(x: &'a str) {\n    let y = x;\n}";
        let chunks = chunk_syntax(src, 40, 100, 5);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].1, 3);
    }

    #[test]
    fn syntax_empty_file() {
        assert!(chunk_syntax("", 40, 100, 5).is_empty());
    }

    #[test]
    fn syntax_template_string_braces_ignored() {
        // TS 反引号模板串内 ${...} 花括号不计 → 单段
        let src = "function f() {\n    const s = `a ${ x } b`;\n    return s;\n}";
        let chunks = chunk_syntax(src, 40, 100, 5);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].1, 4);
    }

    #[test]
    fn cosine_identical_and_orthogonal() {
        assert!((cosine(&[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
    }

    #[test]
    fn cosine_handles_bad_input() {
        assert_eq!(cosine(&[1.0], &[1.0, 2.0]), 0.0); // length mismatch
        assert_eq!(cosine(&[], &[]), 0.0); // empty
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 1.0]), 0.0); // zero vector
    }

    #[test]
    fn chunk_windows_and_overlap() {
        let content = (1..=100).map(|n| n.to_string()).collect::<Vec<_>>().join("\n");
        let chunks = chunk_lines(&content, 40, 5);
        assert_eq!(chunks[0].0, 1);
        assert_eq!(chunks[0].1, 40);
        // step = 40 - 5 = 35, so second window starts at line 36.
        assert_eq!(chunks[1].0, 36);
        // last window ends exactly at the final line.
        assert_eq!(chunks.last().unwrap().1, 100);
    }

    #[test]
    fn chunk_empty_file() {
        assert!(chunk_lines("", 40, 5).is_empty());
    }

    #[test]
    fn chunk_smaller_than_window() {
        let chunks = chunk_lines("a\nb\nc", 40, 5);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], (1, 3, "a\nb\nc".to_string()));
    }

    #[test]
    fn blob_roundtrip() {
        let v = vec![0.5f32, -1.25, 3.0, 0.0];
        assert_eq!(blob_to_vec(&vec_to_blob(&v)), v);
    }

    #[test]
    fn top_k_orders_and_truncates() {
        let scores = [0.1, 0.9, 0.5, 0.7];
        assert_eq!(top_k_indices(&scores, 2), vec![1, 3]);
        assert_eq!(top_k_indices(&scores, 10).len(), 4); // k larger than input
    }

    fn temp_ws(tag: &str) -> String {
        let mut p = std::env::temp_dir();
        let uniq = format!(
            "meyatu_sem_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        p.push(uniq);
        std::fs::create_dir_all(&p).unwrap();
        p.to_string_lossy().to_string()
    }

    fn insert_chunk(conn: &Connection, path: &str) {
        conn.execute(
            "INSERT INTO chunks (path, start_line, end_line, content, vector, mtime) VALUES (?1,1,2,'x',?2,1)",
            rusqlite::params![path, vec_to_blob(&[0.1f32, 0.2])],
        )
        .unwrap();
    }

    #[test]
    fn needs_full_rebuild_logic() {
        assert!(!needs_full_rebuild(None, "m"));          // first index
        assert!(!needs_full_rebuild(Some("m"), "m"));     // same model
        assert!(needs_full_rebuild(Some("old"), "m"));    // changed
    }

    #[test]
    fn ensure_model_wipes_on_change() {
        let ws = temp_ws("model_change");
        {
            let conn = open_db(&ws).unwrap();
            insert_chunk(&conn, "a.rs");
            meta_set(&conn, "embed_model", "old-model").unwrap();
        }
        ensure_index_meta(&ws, DEFAULT_EMBED_MODEL, CHUNK_VERSION).unwrap();
        let conn = open_db(&ws).unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
        assert_eq!(meta_get(&conn, "embed_model").as_deref(), Some(DEFAULT_EMBED_MODEL));
    }

    #[test]
    fn ensure_model_keeps_on_same_model() {
        let ws = temp_ws("model_same");
        {
            let conn = open_db(&ws).unwrap();
            insert_chunk(&conn, "a.rs");
            meta_set(&conn, "embed_model", DEFAULT_EMBED_MODEL).unwrap();
            meta_set(&conn, "chunk_version", CHUNK_VERSION).unwrap();
        }
        ensure_index_meta(&ws, DEFAULT_EMBED_MODEL, CHUNK_VERSION).unwrap();
        let conn = open_db(&ws).unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn config_roundtrip_and_default() {
        let dir = std::env::temp_dir().join(format!(
            "meyatu_sem_cfg_{}_{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("semantic.json");

        // missing file → default
        let d = read_config_at(&p);
        assert_eq!(d.embed_base, DEFAULT_EMBED_BASE);
        assert_eq!(d.embed_model, DEFAULT_EMBED_MODEL);

        // write + read back
        write_config_at(&p, &SemanticConfig {
            embed_base: "http://localhost:1234".into(),
            embed_model: "custom-embed".into(),
        }).unwrap();
        let r = read_config_at(&p);
        assert_eq!(r.embed_base, "http://localhost:1234");
        assert_eq!(r.embed_model, "custom-embed");
    }

    #[test]
    fn dim_mismatch_logic() {
        assert!(!dim_mismatch(768, None));        // 无记录 → 不守卫
        assert!(!dim_mismatch(768, Some(768)));   // 一致
        assert!(dim_mismatch(768, Some(1024)));   // 不符
    }

    #[test]
    fn write_index_records_dim() {
        let ws = temp_ws("dim_write");
        let files = vec![FileChunks {
            path: format!("{ws}/f.rs"),
            mtime: 1,
            chunks: vec![(1, 2, "x\ny".to_string())],
        }];
        let vectors = vec![vec![0.1f32, 0.2, 0.3]];
        let written = write_index(&ws, files, vectors).unwrap();
        assert_eq!(written, 1);
        assert_eq!(read_embed_dim(&ws), Some(3));
    }

    #[test]
    fn orphan_paths_lists_missing() {
        let stored = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let live: HashSet<String> = ["a", "c"].iter().map(|s| s.to_string()).collect();
        let mut o = orphan_paths(&stored, &live);
        o.sort();
        assert_eq!(o, vec!["b".to_string()]);
    }

    #[test]
    fn orphan_paths_empty_when_all_live() {
        let stored = vec!["a".to_string(), "b".to_string()];
        let live: HashSet<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        assert!(orphan_paths(&stored, &live).is_empty());
    }

    #[test]
    fn reconcile_removes_orphans() {
        let ws = temp_ws("reconcile");
        {
            let conn = open_db(&ws).unwrap();
            insert_chunk(&conn, "live1.rs");
            insert_chunk(&conn, "live2.rs");
            insert_chunk(&conn, "gone.rs");
        }
        let live: HashSet<String> =
            ["live1.rs", "live2.rs"].iter().map(|s| s.to_string()).collect();
        let removed = reconcile_deletions(&ws, &live).unwrap();
        assert_eq!(removed, 1);
        let conn = open_db(&ws).unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(DISTINCT path) FROM chunks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
        let gone: i64 = conn
            .query_row("SELECT COUNT(*) FROM chunks WHERE path = 'gone.rs'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(gone, 0);
    }

    #[test]
    fn status_no_db_reports_not_indexed() {
        let ws = std::env::temp_dir()
            .join(format!("meyatu_sem_status_nodb_{}_{}", std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()))
            .to_string_lossy().into_owned();
        std::fs::create_dir_all(&ws).unwrap();
        let json = tool_semantic_status(ws.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["indexed"], false);
        assert_eq!(v["file_count"], 0);
        assert_eq!(v["chunk_count"], 0);
        assert!(v["embed_model"].is_null());
    }

    #[test]
    fn status_reports_counts_after_write() {
        let ws = std::env::temp_dir()
            .join(format!("meyatu_sem_status_data_{}_{}", std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()))
            .to_string_lossy().into_owned();
        std::fs::create_dir_all(&ws).unwrap();
        let conn = open_db(&ws).unwrap();
        conn.execute("INSERT INTO chunks (path,start_line,end_line,content,vector,mtime) VALUES ('a.rs',0,1,'x',x'00',0)", []).unwrap();
        conn.execute("INSERT INTO chunks (path,start_line,end_line,content,vector,mtime) VALUES ('b.rs',0,1,'y',x'00',0)", []).unwrap();
        meta_set(&conn, "embed_model", "nomic-embed-text").unwrap();
        meta_set(&conn, "embed_dim", "768").unwrap();
        drop(conn);

        let json = tool_semantic_status(ws.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["indexed"], true);
        assert_eq!(v["file_count"], 2);
        assert_eq!(v["chunk_count"], 2);
        assert_eq!(v["embed_model"], "nomic-embed-text");
        assert_eq!(v["embed_dim"], 768);
    }

    #[test]
    fn brace_ext_routing() {
        assert!(is_brace_ext("/x/foo.rs"));
        assert!(is_brace_ext("/x/foo.tsx"));
        assert!(is_brace_ext("a.go"));
        assert!(!is_brace_ext("/x/foo.py"));
        assert!(!is_brace_ext("README.md"));
        assert!(!is_brace_ext("/x/Makefile")); // 无扩展名
    }

    #[test]
    fn build_index_plan_uses_syntax_for_brace_files() {
        // .rs 文件应走 chunk_syntax：结果与直接调用一致
        let ws = temp_ws("route_rs");
        let mut src = String::from("fn big() {\n");
        for i in 0..148 {
            src.push_str(&format!("    let x{i} = {i};\n"));
        }
        src.push('}'); // 150 行 → 超 max，会被 chunk_syntax 子切成多段
        let fpath = format!("{ws}/big.rs");
        std::fs::write(&fpath, &src).unwrap();
        let (files, _skipped, _live) = build_index_plan(&ws).unwrap();
        let f = files.iter().find(|f| f.path == fpath).expect("big.rs indexed");
        let expected = chunk_syntax(&src, CHUNK_TARGET, CHUNK_MAX, CHUNK_OVERLAP);
        assert_eq!(f.chunks.len(), expected.len());
        assert!(f.chunks.len() > 1, "150-line fn must sub-split, not 1 window-chunk");
    }

    #[test]
    fn cv_needs_rebuild_logic() {
        assert!(cv_needs_rebuild(None, "syntax-v1"));            // 旧库无键 → 重建
        assert!(!cv_needs_rebuild(Some("syntax-v1"), "syntax-v1")); // 已迁移
        assert!(cv_needs_rebuild(Some("old"), "syntax-v1"));     // 版本变更
    }

    #[test]
    fn ensure_index_meta_wipes_on_chunk_version_change() {
        let ws = temp_ws("cv_change");
        {
            let conn = open_db(&ws).unwrap();
            insert_chunk(&conn, "a.rs");
            meta_set(&conn, "embed_model", DEFAULT_EMBED_MODEL).unwrap();
            meta_set(&conn, "chunk_version", "old-strategy").unwrap();
        }
        ensure_index_meta(&ws, DEFAULT_EMBED_MODEL, CHUNK_VERSION).unwrap();
        let conn = open_db(&ws).unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0); // chunk_version 变更 → 清库
        assert_eq!(meta_get(&conn, "chunk_version").as_deref(), Some(CHUNK_VERSION));
    }

    #[test]
    fn ensure_index_meta_keeps_when_all_match() {
        let ws = temp_ws("cv_same");
        {
            let conn = open_db(&ws).unwrap();
            insert_chunk(&conn, "a.rs");
            meta_set(&conn, "embed_model", DEFAULT_EMBED_MODEL).unwrap();
            meta_set(&conn, "chunk_version", CHUNK_VERSION).unwrap();
        }
        ensure_index_meta(&ws, DEFAULT_EMBED_MODEL, CHUNK_VERSION).unwrap();
        let conn = open_db(&ws).unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1); // model + cv 均符 → 保留
    }

    #[test]
    fn generation_default_and_bump() {
        let ws = temp_ws("gen_bump");
        let conn = open_db(&ws).unwrap();
        assert_eq!(read_generation(&conn), 0);
        bump_generation(&conn).unwrap();
        assert_eq!(read_generation(&conn), 1);
        bump_generation(&conn).unwrap();
        assert_eq!(read_generation(&conn), 2);
    }

    #[test]
    fn write_index_bumps_generation() {
        let ws = temp_ws("gen_write");
        let before = { let c = open_db(&ws).unwrap(); read_generation(&c) };
        let files = vec![FileChunks {
            path: format!("{ws}/f.rs"),
            mtime: 1,
            chunks: vec![(1, 2, "x\ny".to_string())],
        }];
        write_index(&ws, files, vec![vec![0.1f32, 0.2, 0.3]]).unwrap();
        let after = { let c = open_db(&ws).unwrap(); read_generation(&c) };
        assert!(after > before);
    }

    #[test]
    fn reconcile_bumps_generation_only_when_removed() {
        let ws = temp_ws("gen_reconcile");
        {
            let conn = open_db(&ws).unwrap();
            insert_chunk(&conn, "live.rs");
            insert_chunk(&conn, "gone.rs");
        }
        let g0 = { let c = open_db(&ws).unwrap(); read_generation(&c) };
        let live: HashSet<String> = ["live.rs"].iter().map(|s| s.to_string()).collect();
        assert_eq!(reconcile_deletions(&ws, &live).unwrap(), 1); // gone.rs orphaned
        let g1 = { let c = open_db(&ws).unwrap(); read_generation(&c) };
        assert!(g1 > g0); // removed>0 → bump
        assert_eq!(reconcile_deletions(&ws, &live).unwrap(), 0); // nothing to remove
        let g2 = { let c = open_db(&ws).unwrap(); read_generation(&c) };
        assert_eq!(g2, g1); // removed==0 → no bump
    }

    #[test]
    fn ensure_index_meta_bumps_generation_on_wipe() {
        let ws = temp_ws("gen_ensure");
        {
            let conn = open_db(&ws).unwrap();
            insert_chunk(&conn, "a.rs");
            meta_set(&conn, "embed_model", "old-model").unwrap();
            meta_set(&conn, "chunk_version", CHUNK_VERSION).unwrap();
        }
        let g0 = { let c = open_db(&ws).unwrap(); read_generation(&c) };
        ensure_index_meta(&ws, DEFAULT_EMBED_MODEL, CHUNK_VERSION).unwrap(); // model change → wipe
        let g1 = { let c = open_db(&ws).unwrap(); read_generation(&c) };
        assert!(g1 > g0);
        ensure_index_meta(&ws, DEFAULT_EMBED_MODEL, CHUNK_VERSION).unwrap(); // no change → no wipe
        let g2 = { let c = open_db(&ws).unwrap(); read_generation(&c) };
        assert_eq!(g2, g1);
    }
}
