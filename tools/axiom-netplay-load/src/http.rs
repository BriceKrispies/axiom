//! A minimal async HTTP/1.1 client for the server's JSON control endpoints
//! (`/matchmake`, `/readyz`, `/metrics`).
//!
//! Deliberately tiny: the load tool only POSTs/GETs a handful of small JSON
//! objects, so a full HTTP client dependency (`reqwest` + `hyper` + TLS) is not
//! worth pulling in. It opens a one-shot `Connection: close` socket, reads the
//! whole response, and parses the body as JSON. This is repo tooling; re-using a
//! hand-rolled HTTP path here is fine — unlike the binary game protocol, which is
//! shared with the engine and must never be re-implemented.

use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Perform one request and return `(status_code, json_body)`. The body is parsed
/// even on a non-2xx status, so callers can read a `503` readiness/error report.
pub async fn request(base: &str, method: &str, path: &str) -> Result<(u16, Value), String> {
    let (host, port) = authority(base)?;
    let connect = TcpStream::connect((host.as_str(), port));
    let mut stream = tokio::time::timeout(Duration::from_secs(5), connect)
        .await
        .map_err(|_| format!("connect to {host}:{port} timed out"))?
        .map_err(|e| format!("connect to {host}:{port} failed: {e}"))?;

    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(req.as_bytes())
        .await
        .map_err(|e| format!("write {path} failed: {e}"))?;

    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut buf))
        .await
        .map_err(|_| format!("read {path} timed out"))?
        .map_err(|e| format!("read {path} failed: {e}"))?;

    let status = status_code(&buf)?;
    let body = body_slice(&buf)?;
    let json = parse_json(body)
        .map_err(|e| format!("{path}: {e}; body={:?}", String::from_utf8_lossy(body)))?;
    Ok((status, json))
}

/// `POST` `path` (empty body) and return `(status, json)`.
pub async fn post_json(base: &str, path: &str) -> Result<(u16, Value), String> {
    request(base, "POST", path).await
}

/// `GET` `path` and return `(status, json)`.
pub async fn get_json(base: &str, path: &str) -> Result<(u16, Value), String> {
    request(base, "GET", path).await
}

/// Split an `http://host:port[/...]` base into `(host, port)`.
fn authority(base: &str) -> Result<(String, u16), String> {
    let rest = base
        .strip_prefix("http://")
        .ok_or_else(|| format!("only http:// targets are supported: {base:?}"))?;
    let auth = rest.split('/').next().unwrap_or(rest);
    let (host, port) = auth.split_once(':').unwrap_or((auth, "80"));
    let port = port
        .parse::<u16>()
        .map_err(|_| format!("bad port in {base:?}"))?;
    Ok((host.to_string(), port))
}

/// Parse the numeric status code out of the `HTTP/1.1 <code> <reason>` line.
fn status_code(raw: &[u8]) -> Result<u16, String> {
    let head = raw.split(|&b| b == b'\r').next().unwrap_or(raw);
    let text = String::from_utf8_lossy(head);
    text.split_whitespace()
        .nth(1)
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or_else(|| format!("malformed status line: {text:?}"))
}

/// The bytes after the blank `\r\n\r\n` that ends the headers.
fn body_slice(raw: &[u8]) -> Result<&[u8], String> {
    raw.windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| &raw[i + 4..])
        .ok_or_else(|| "no header terminator in response".to_string())
}

/// Parse a JSON object from a response body. Tolerates `Transfer-Encoding:
/// chunked` framing (Kestrel uses Content-Length for these tiny payloads, but
/// the fallback keeps the tool robust): on a direct parse failure, retry on the
/// slice from the first `{` to the last `}`.
fn parse_json(body: &[u8]) -> Result<Value, String> {
    serde_json::from_slice::<Value>(body)
        .or_else(|_| serde_json::from_slice::<Value>(inner_object(body)))
        .map_err(|e| format!("not JSON: {e}"))
}

/// The slice from the first `{` to the last `}` inclusive (or the whole input if
/// either is absent) — strips chunk-size/terminator lines around one object.
fn inner_object(body: &[u8]) -> &[u8] {
    let start = body.iter().position(|&b| b == b'{');
    let end = body.iter().rposition(|&b| b == b'}');
    match (start, end) {
        (Some(s), Some(e)) if e >= s => &body[s..=e],
        _ => body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority_splits_host_and_port() {
        assert_eq!(
            authority("http://localhost:8090").unwrap(),
            ("localhost".into(), 8090)
        );
        assert_eq!(
            authority("http://127.0.0.1:8101/matchmake").unwrap(),
            ("127.0.0.1".into(), 8101)
        );
    }

    #[test]
    fn authority_rejects_non_http() {
        assert!(authority("ws://host/ws").is_err());
        assert!(authority("http://host:notaport").is_err());
    }

    #[test]
    fn status_and_body_are_extracted() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"roomId\":\"mm-1\"}";
        assert_eq!(status_code(raw).unwrap(), 200);
        assert_eq!(body_slice(raw).unwrap(), b"{\"roomId\":\"mm-1\"}");
    }

    #[test]
    fn status_line_must_be_well_formed() {
        assert!(status_code(b"garbage\r\n\r\n").is_err());
    }

    #[test]
    fn missing_terminator_is_an_error() {
        assert!(body_slice(b"HTTP/1.1 200 OK\r\nno body").is_err());
    }

    #[test]
    fn parses_plain_json() {
        let v = parse_json(b"{\"roomId\":\"mm-2\",\"nodeUrl\":\"ws://n/ws\"}").unwrap();
        assert_eq!(v["roomId"], "mm-2");
        assert_eq!(v["nodeUrl"], "ws://n/ws");
    }

    #[test]
    fn parses_chunk_framed_json() {
        // A single chunk: size line, object, terminator — the fallback recovers it.
        let body = b"1f\r\n{\"roomId\":\"mm-3\"}\r\n0\r\n\r\n";
        let v = parse_json(body).unwrap();
        assert_eq!(v["roomId"], "mm-3");
    }

    #[test]
    fn inner_object_falls_back_to_whole_input() {
        assert_eq!(inner_object(b"no braces"), b"no braces");
    }
}
