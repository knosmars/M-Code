//! Web research tools: scrape a single page, BFS-crawl a site, or map links.
//!
//! Permissive (MIT) stack: `reqwest` fetches HTML, `htmd` converts it to
//! Markdown. JS-heavy SPA pages (content injected client-side) degrade to
//! whatever the server returns in the initial HTML — there is no headless
//! browser in this build.

use std::collections::HashSet;
use std::time::Duration;

const USER_AGENT: &str = "meyatu/1.0";

/// Fetch a URL and return its raw HTML body. Errors carry a short reason.
async fn fetch_html(url: &str, timeout_ms: u64) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .map_err(|e| format!("Client init: {e}"))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Fetch failed: {e}"))?;

    resp.text().await.map_err(|e| format!("Read body failed: {e}"))
}

/// Strip a `<tag>…</tag>` (and self-contained variants) from HTML, case-insensitive.
/// Rust's regex has no backreferences, so each tag is removed explicitly.
fn strip_tag(html: &str, tag: &str) -> String {
    let pattern = format!(r"(?is)<{tag}\b[^>]*>.*?</{tag}>");
    match regex::Regex::new(&pattern) {
        Ok(re) => re.replace_all(html, "").into_owned(),
        Err(_) => html.to_string(),
    }
}

/// Convert an HTML document to Markdown, dropping non-content tags first.
fn html_to_markdown(html: &str) -> String {
    let mut cleaned = strip_tag(html, "script");
    cleaned = strip_tag(&cleaned, "style");
    cleaned = strip_tag(&cleaned, "noscript");
    // htmd::convert only errors on malformed input; fall back to the cleaned
    // HTML so we never silently drop a page's content.
    htmd::convert(&cleaned).unwrap_or(cleaned)
}

/// Scrape a single URL and return its contents as Markdown.
#[tauri::command]
pub async fn tool_web_scrape(url: String) -> Result<String, String> {
    let html = fetch_html(&url, 15_000).await?;
    Ok(html_to_markdown(&html))
}

/// Crawl multiple pages from a starting URL using same-domain BFS.
#[tauri::command]
pub async fn tool_web_crawl(url: String, max_pages: Option<u32>) -> Result<String, String> {
    let max = max_pages.unwrap_or(5) as usize;
    let mut output = String::new();
    let mut to_visit: Vec<String> = vec![url.clone()];
    let mut visited: HashSet<String> = HashSet::new();

    while visited.len() < max {
        let current = match to_visit.pop() {
            Some(u) => u,
            None => break,
        };
        if !visited.insert(current.clone()) {
            continue;
        }

        if let Ok(html) = fetch_html(&current, 10_000).await {
            let md = html_to_markdown(&html);
            output.push_str(&format!("\n\n--- {current} ---\n\n{md}"));

            for link in extract_links(&html, &current) {
                if !visited.contains(&link) {
                    to_visit.push(link);
                }
            }
        }
    }

    Ok(output)
}

/// Discover same-domain URLs on a page.
#[tauri::command]
pub async fn tool_web_map(url: String, limit: Option<u32>) -> Result<String, String> {
    let max = limit.unwrap_or(20) as usize;
    let html = fetch_html(&url, 10_000).await?;
    let found: Vec<String> = extract_links(&html, &url).into_iter().take(max).collect();
    Ok(serde_json::to_string(&found).unwrap_or_else(|_| "[]".to_string()))
}

/// Extract HTTP/HTTPS links from HTML anchor tags, resolved + same-domain filtered.
fn extract_links(html: &str, base: &str) -> Vec<String> {
    let re = regex::Regex::new(r#"<a[^>]*href="([^"]+)""#).ok();
    let Some(re) = re else { return vec![] };
    let base_url = url::Url::parse(base).ok();
    let Some(base_url) = base_url else { return vec![] };

    let mut links = Vec::new();
    for cap in re.captures_iter(html) {
        let href = cap.get(1).map_or("", |m| m.as_str());
        let resolved = if href.starts_with('/') {
            base_url.join(href).ok().map(|u| u.to_string())
        } else if href.starts_with("http://") || href.starts_with("https://") {
            Some(href.to_string())
        } else {
            None
        };
        if let Some(url_str) = resolved {
            if let Ok(parsed) = url::Url::parse(&url_str) {
                // Keep same-domain links
                if parsed.host_str() == base_url.host_str() {
                    links.push(url_str);
                }
            }
        }
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_keeps_text_and_drops_scripts_styles() {
        let html = r#"<html><head><style>.x{color:red}</style></head>
            <body><script>evil()</script><h1>Title</h1><p>Hello world</p></body></html>"#;
        let md = html_to_markdown(html);
        assert!(md.contains("Title"), "heading text kept: {md:?}");
        assert!(md.contains("Hello world"), "paragraph text kept: {md:?}");
        assert!(!md.contains("evil()"), "script body dropped: {md:?}");
        assert!(!md.contains("color:red"), "style body dropped: {md:?}");
    }

    #[test]
    fn extract_links_resolves_relative_and_filters_cross_domain() {
        let html = r#"
            <a href="/docs/a">rel</a>
            <a href="https://example.com/b">abs same</a>
            <a href="https://other.com/c">abs other</a>
            <a href="mailto:x@y.z">mail</a>
        "#;
        let links = extract_links(html, "https://example.com/start");
        assert!(links.contains(&"https://example.com/docs/a".to_string()));
        assert!(links.contains(&"https://example.com/b".to_string()));
        assert!(!links.iter().any(|l| l.contains("other.com")), "cross-domain filtered");
        assert!(!links.iter().any(|l| l.contains("mailto")), "non-http filtered");
    }
}
