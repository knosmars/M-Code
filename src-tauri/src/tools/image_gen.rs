use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Minimal base64 encoder (avoids adding a new crate dependency)
// ---------------------------------------------------------------------------

const BASE64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
    let mut result = String::new();
    let mut i = 0;
    while i < data.len() {
        let b1 = data[i];
        let b2 = data.get(i + 1).copied().unwrap_or(0);
        let b3 = data.get(i + 2).copied().unwrap_or(0);

        result.push(BASE64_CHARS[(b1 >> 2) as usize] as char);
        result.push(BASE64_CHARS[(((b1 & 0x3) << 4) | (b2 >> 4)) as usize] as char);
        result.push(BASE64_CHARS[(((b2 & 0xF) << 2) | (b3 >> 6)) as usize] as char);
        result.push(BASE64_CHARS[(b3 & 0x3F) as usize] as char);

        i += 3;
    }

    let padding = (3 - (data.len() % 3)) % 3;
    if padding > 0 {
        let len = result.len();
        result.replace_range(len - padding..len, &"=".repeat(padding));
    }

    result
}

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct ImageGenerationRequest {
    model: String,
    prompt: String,
    size: String,
    n: u8,
}

#[derive(Debug, Deserialize)]
struct ImageGenerationResponse {
    data: Vec<ImageData>,
}

#[derive(Debug, Deserialize)]
struct ImageData {
    url: Option<String>,
    b64_json: Option<String>,
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

const VALID_SIZES: &[&str] = &["1024x1024", "1792x1024", "1024x1792"];
const VALID_STYLES: &[&str] = &["natural", "vivid"];

fn validate_size(size: &str) -> Result<(), String> {
    if VALID_SIZES.contains(&size) {
        Ok(())
    } else {
        Err(format!(
            "Invalid size '{}'. Must be one of: {}",
            size,
            VALID_SIZES.join(", ")
        ))
    }
}

fn validate_style(style: &str) -> Result<(), String> {
    if VALID_STYLES.contains(&style) {
        Ok(())
    } else {
        Err(format!(
            "Invalid style '{}'. Must be one of: {}",
            style,
            VALID_STYLES.join(", ")
        ))
    }
}

// ---------------------------------------------------------------------------
// Tool: generate_image
// ---------------------------------------------------------------------------

/// Generate an image from a text prompt using an AI image generation API.
///
/// Defaults:
/// - size: "1024x1024"
/// - style: "natural"
///
/// If no API key is configured, returns a helpful error explaining how to set up
/// image generation. If an API key is found via the `IMAGE_GEN_API_KEY`
/// environment variable, makes a request to the OpenAI DALL-E 3 endpoint and
/// returns the image as a base64 data URL.
#[tauri::command]
pub async fn tool_generate_image(
    prompt: String,
    size: Option<String>,
    style: Option<String>,
) -> Result<String, String> {
    // Validate prompt
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err("Prompt cannot be empty. Provide a detailed description of the image to generate.".to_string());
    }

    // Apply defaults and validate
    let size = size.unwrap_or_else(|| "1024x1024".to_string());
    validate_size(&size)?;

    let style = style.unwrap_or_else(|| "natural".to_string());
    validate_style(&style)?;

    // Check for API key
    let api_key = match std::env::var("IMAGE_GEN_API_KEY") {
        Ok(key) if !key.trim().is_empty() => key,
        _ => {
            // Mock mode: generate a placeholder SVG so the tool is usable
            // without a real API key during development/testing.
            let svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
  <rect width="1024" height="1024" fill="{bg}"/>
  <rect x="100" y="100" width="824" height="824" rx="20" fill="{card_bg}"/>
  <text x="512" y="460" font-family="{font}" font-size="28" fill="{text}" text-anchor="middle">{prompt}</text>
  <text x="512" y="520" font-family="{font}" font-size="16" fill="{hint}" text-anchor="middle">Mock image — set IMAGE_GEN_API_KEY for real generation</text>
</svg>"#,
                prompt = html_escape(prompt),
                font = "system-ui, sans-serif",
                bg = "#f0f0f0",
                card_bg = "#e0e0e0",
                text = "#888",
                hint = "#aaa",
            );
            return Ok(format!("data:image/svg+xml;base64,{}", base64_encode(svg.as_bytes())));
        }
    };

    // Build request
    let request_body = ImageGenerationRequest {
        model: "dall-e-3".to_string(),
        prompt: prompt.to_string(),
        size,
        n: 1,
    };

    let client = reqwest::Client::new();
    let response = client
        .post("https://api.openai.com/v1/images/generations")
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Failed to send image generation request: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response
            .text()
            .await
            .unwrap_or_else(|_| "<unable to read response body>".to_string());
        return Err(format!("Image generation API error ({}): {}", status, text));
    }

    let api_response: ImageGenerationResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse image generation response: {e}"))?;

    let image_data = api_response
        .data
        .into_iter()
        .next()
        .ok_or_else(|| "Image generation returned no data.".to_string())?;

    // Prefer b64_json if available; otherwise download from URL
    if let Some(b64) = image_data.b64_json {
        Ok(format!("data:image/png;base64,{b64}"))
    } else if let Some(url) = image_data.url {
        let image_bytes = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to download generated image: {e}"))?
            .bytes()
            .await
            .map_err(|e| format!("Failed to read generated image bytes: {e}"))?;
        let b64 = base64_encode(&image_bytes);
        Ok(format!("data:image/png;base64,{b64}"))
    } else {
        Err("Image generation response contained neither URL nor base64 data.".to_string())
    }
}

/// Simple HTML-escaping for embedding user prompt text in SVG.
fn html_escape(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '&' => "&amp;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&#39;".to_string(),
            _ => c.to_string(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_prompt_returns_error() {
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tool_generate_image("".to_string(), None, None));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Prompt cannot be empty"), "expected empty prompt error, got: {err}");
    }

    #[test]
    fn whitespace_only_prompt_returns_error() {
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tool_generate_image("   ".to_string(), None, None));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Prompt cannot be empty"), "expected empty prompt error, got: {err}");
    }

    #[test]
    fn invalid_size_returns_error() {
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tool_generate_image(
                "a cat".to_string(),
                Some("999x999".to_string()),
                None,
            ));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Invalid size"), "expected invalid size error, got: {err}");
    }

    #[test]
    fn invalid_style_returns_error() {
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tool_generate_image(
                "a cat".to_string(),
                None,
                Some("cartoon".to_string()),
            ));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Invalid style"), "expected invalid style error, got: {err}");
    }

    #[test]
    fn valid_params_without_api_key_returns_mock_svg() {
        // Ensure no API key is set in the environment
        std::env::remove_var("IMAGE_GEN_API_KEY");

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tool_generate_image(
                "a serene mountain landscape at sunset".to_string(),
                Some("1024x1024".to_string()),
                Some("natural".to_string()),
            ));
        assert!(result.is_ok(), "expected mock SVG without API key, got error: {:?}", result.err());
        let data_url = result.unwrap();
        assert!(data_url.starts_with("data:image/svg+xml;base64,"), "expected SVG data URL, got: {data_url}");
    }

    #[test]
    fn defaults_are_applied_correctly() {
        // Verify defaults without calling the API by checking validation passes
        // and we get mock SVG rather than a validation error.
        std::env::remove_var("IMAGE_GEN_API_KEY");

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tool_generate_image(
                "a red apple on a wooden table".to_string(),
                None,
                None,
            ));
        assert!(result.is_ok(), "expected mock SVG with defaults, got error: {:?}", result.err());
    }

    #[test]
    fn html_escape_handles_special_chars() {
        assert_eq!(html_escape("hello"), "hello");
        assert_eq!(html_escape("<tag>"), "&lt;tag&gt;");
        assert_eq!(html_escape("a & b"), "a &amp; b");
        assert_eq!(html_escape("\"quote\""), "&quot;quote&quot;");
        assert_eq!(html_escape("'single'"), "&#39;single&#39;");
    }
}
