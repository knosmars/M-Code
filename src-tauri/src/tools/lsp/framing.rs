//! LSP stdio framing: `Content-Length: N\r\n\r\n<N bytes JSON>`.
//! Pure and synchronous; the transport layer for the LSP client.

use serde_json::Value;
use std::io::BufRead;

/// Serialize `value` to a single Content-Length framed message.
pub fn encode(value: &Value) -> Vec<u8> {
    let body = serde_json::to_vec(value).unwrap_or_default();
    let mut frame = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    frame.extend_from_slice(&body);
    frame
}

/// Read one framed message: parse headers until the blank line, read exactly
/// `Content-Length` bytes, parse JSON. Handles split reads via `BufRead`.
pub fn decode<R: BufRead>(reader: &mut R) -> Result<Value, String> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .map_err(|e| format!("lsp read header failed: {e}"))?;
        if n == 0 {
            return Err("lsp server closed its output".to_string());
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break; // end of headers
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse::<usize>().ok();
        }
        // other headers (e.g. Content-Type) are ignored
    }
    let len = content_length.ok_or("lsp frame missing Content-Length")?;
    let mut body = vec![0u8; len];
    reader
        .read_exact(&mut body)
        .map_err(|e| format!("lsp read body failed: {e}"))?;
    serde_json::from_slice(&body).map_err(|e| format!("lsp bad json body: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::{BufReader, Read};

    /// A reader that yields at most one byte per `read`, to exercise split
    /// reads in header parsing and body reads.
    struct OneByteReader<'a> {
        data: &'a [u8],
        pos: usize,
    }
    impl<'a> Read for OneByteReader<'a> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.pos >= self.data.len() || buf.is_empty() {
                return Ok(0);
            }
            buf[0] = self.data[self.pos];
            self.pos += 1;
            Ok(1)
        }
    }

    #[test]
    fn encode_has_correct_header_and_body() {
        let frame = encode(&json!({ "a": 1 }));
        let s = String::from_utf8(frame).unwrap();
        assert!(s.starts_with("Content-Length: 7\r\n\r\n"));
        assert!(s.ends_with("{\"a\":1}"));
    }

    #[test]
    fn decode_single_frame() {
        let frame = encode(&json!({ "id": 3, "result": {} }));
        let mut r = BufReader::new(&frame[..]);
        let v = decode(&mut r).unwrap();
        assert_eq!(v["id"], 3);
    }

    #[test]
    fn decode_handles_split_reads() {
        let frame = encode(&json!({ "id": 9, "method": "x" }));
        let mut r = BufReader::new(OneByteReader { data: &frame, pos: 0 });
        let v = decode(&mut r).unwrap();
        assert_eq!(v["id"], 9);
        assert_eq!(v["method"], "x");
    }

    #[test]
    fn decode_back_to_back_frames() {
        let mut buf = encode(&json!({ "id": 1 }));
        buf.extend(encode(&json!({ "id": 2 })));
        let mut r = BufReader::new(&buf[..]);
        assert_eq!(decode(&mut r).unwrap()["id"], 1);
        assert_eq!(decode(&mut r).unwrap()["id"], 2);
    }

    #[test]
    fn decode_ignores_extra_headers() {
        let body = b"{\"id\":5}";
        let mut frame =
            format!("Content-Type: application/vscode-jsonrpc\r\nContent-Length: {}\r\n\r\n", body.len())
                .into_bytes();
        frame.extend_from_slice(body);
        let mut r = BufReader::new(&frame[..]);
        assert_eq!(decode(&mut r).unwrap()["id"], 5);
    }

    #[test]
    fn decode_missing_content_length_errors() {
        let mut r = BufReader::new(&b"X-Foo: bar\r\n\r\n"[..]);
        assert!(decode(&mut r).is_err());
    }
}
