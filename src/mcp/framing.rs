//! The MCP wire format: messages framed by a `Content-Length` header, as LSP
//! frames them, and the JSON-RPC replies written in it.

use std::io::{self, BufRead, Write};

use serde_json::{Value, json};

/// Helper: send a successful tool result.
pub(super) fn send_tool_result(w: &mut impl Write, id: &Value, text: &str) -> io::Result<()> {
    send_result(w, id, json!({"content": [{"type": "text", "text": text}]}))
}

/// Helper: send a tool error.
pub(super) fn send_tool_error(w: &mut impl Write, id: &Value, text: &str) -> io::Result<()> {
    send_result(
        w,
        id,
        json!({"isError": true, "content": [{"type": "text", "text": text}]}),
    )
}

/// Upper bound on a single MCP message body. The header's `Content-Length`
/// is untrusted; we refuse anything larger rather than pre-allocate it.
const MAX_LSP_MESSAGE_BYTES: usize = 64 * 1024 * 1024;

pub(super) fn read_lsp_message(reader: &mut impl BufRead) -> io::Result<String> {
    let mut content_length: Option<usize> = None;
    let mut header = String::new();
    loop {
        header.clear();
        let n = reader.read_line(&mut header)?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "stdin closed"));
        }
        let trimmed = header.trim();
        if trimmed.is_empty() {
            break;
        }
        // A "header" line that's actually a JSON body (and we haven't seen a
        // Content-Length yet) means the sender didn't frame the message. Flag
        // it as malformed rather than consuming lines until EOF and dropping it
        // silently — that silent drop is exactly what hid the bare-newline
        // report-status reporter. (Valid frames only reach here with header
        // lines + a blank terminator; the JSON body is read by byte count.)
        if content_length.is_none() && (trimmed.starts_with('{') || trimmed.starts_with('[')) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unframed message: JSON body with no Content-Length header",
            ));
        }
        if let Some(val) = trimmed.strip_prefix("Content-Length:") {
            content_length = val.trim().parse().ok();
        }
    }

    let len = content_length
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length"))?;
    // Cap the declared length before allocating: `len` is attacker/garbage
    // controlled, so `vec![0u8; len]` for a multi-GB Content-Length would
    // abort the whole process on allocation failure. 64 MiB is far above any
    // real MCP message (tool args / file slices) but small enough to refuse.
    if len > MAX_LSP_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Content-Length {len} exceeds {MAX_LSP_MESSAGE_BYTES}-byte cap"),
        ));
    }

    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    String::from_utf8(buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub(super) fn send_message(w: &mut impl Write, body: &str) -> io::Result<()> {
    write!(w, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    w.flush()
}

pub(super) fn send_result(w: &mut impl Write, id: &Value, result: Value) -> io::Result<()> {
    let msg = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    });
    send_message(w, &msg.to_string())
}

pub(super) fn send_error(
    w: &mut impl Write,
    id: Value,
    code: i32,
    message: &str,
) -> io::Result<()> {
    let msg = json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    });
    send_message(w, &msg.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn read_lsp_message_reads_framed_body() {
        let mut c = Cursor::new(b"Content-Length: 5\r\n\r\nhello".to_vec());
        assert_eq!(read_lsp_message(&mut c).unwrap(), "hello");
    }

    // Regression: the `--report-status` hook reporter wrote a BARE
    // newline-delimited JSON line, which the socket server (Content-Length
    // framed) silently dropped — so hook-driven status never reached spyc.
    // The reporter must frame via `send_message` so `read_lsp_message` reads it
    // back; a bare line must NOT parse (that was the bug).
    #[test]
    fn report_status_framing_round_trips_but_a_bare_line_does_not() {
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"report_status","arguments":{"status":"blocked"}}}"#;
        let mut framed = Vec::new();
        send_message(&mut framed, body).unwrap();
        assert_eq!(
            read_lsp_message(&mut Cursor::new(framed)).unwrap(),
            body,
            "send_message framing must round-trip through the socket reader"
        );
        // The old reporter's output: bare JSON + '\n', no Content-Length header.
        let mut bare = Cursor::new(format!("{body}\n").into_bytes());
        let err = read_lsp_message(&mut bare).unwrap_err();
        // Reported as InvalidData ("unframed"), NOT a silent EOF — so the socket
        // server can warn the user instead of dropping it unnoticed.
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("unframed"), "got {err}");
    }

    #[test]
    fn read_lsp_message_rejects_oversized_content_length() {
        // A hostile/garbage header must not trigger a multi-GB allocation;
        // it errors before `vec![0u8; len]`.
        let header = format!("Content-Length: {}\r\n\r\n", u64::from(u32::MAX) * 16);
        let mut c = Cursor::new(header.into_bytes());
        let err = read_lsp_message(&mut c).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("exceeds"));
    }
}
