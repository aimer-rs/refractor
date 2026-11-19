//! How messages go to and from a server: the framing of the protocol (a `Content-Length` header, a blank
//! line and the body) and the JSON-RPC envelopes the bodies are. Nothing here knows what a message is
//! about, so it needs no protocol types.

use std::io::{self, BufRead, Read, Write};

use serde_json::{Value, json};

/// The longest body that is read. A server that says a message is longer than this is not one that is
/// speaking the protocol, and a length that was believed would be a buffer of that size.
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024 * 1024;

/// Writes `body` as one message.
pub fn write_message(out: &mut impl Write, body: &[u8]) -> io::Result<()> {
    write!(out, "Content-Length: {}\r\n\r\n", body.len())?;
    out.write_all(body)?;
    out.flush()
}

/// Reads the next message's body: `None` when the server has closed its end between messages, an error
/// for a message that is cut off or that is not framed as the protocol says.
pub fn read_message(input: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let invalid = |why: &str| io::Error::new(io::ErrorKind::InvalidData, why.to_owned());
    let mut length: Option<usize> = None;
    let mut first_line = true;
    // The headers, one to a line, up to the blank line.
    loop {
        let mut line = Vec::new();
        let read = input.read_until(b'\n', &mut line)?;
        if read == 0 {
            // Closed before a message began, which is the end; closed after one began, which is not.
            return if first_line { Ok(None) } else { Err(io::ErrorKind::UnexpectedEof.into()) };
        }
        first_line = false;
        if !line.ends_with(b"\n") {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        line.pop();
        if line.ends_with(b"\r") {
            line.pop();
        }
        if line.is_empty() {
            break;
        }
        let line = std::str::from_utf8(&line).map_err(|_| invalid("a header that is not text"))?;
        let (name, value) = line.split_once(':').ok_or_else(|| invalid("a header line with no colon"))?;
        if name.trim().eq_ignore_ascii_case("content-length") {
            length = Some(value.trim().parse().map_err(|_| invalid("a Content-Length that is not a length"))?);
        }
    }
    let length = length.ok_or_else(|| invalid("a message with no Content-Length"))?;
    if length > MAX_MESSAGE_BYTES {
        return Err(invalid("a message longer than is read"));
    }
    // The body grows as it arrives, so a length that is claimed and not sent costs nothing.
    let mut body = Vec::new();
    input.by_ref().take(length as u64).read_to_end(&mut body)?;
    if body.len() < length {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    Ok(Some(body))
}

/// What the server answered to a request: the result, or why it could not give one.
#[derive(Clone, Debug, PartialEq)]
pub struct ResponseError {
    pub code: i64,
    pub message: String,
}

/// A message from the server.
#[derive(Clone, Debug, PartialEq)]
pub enum Incoming {
    /// The answer to a request this client made.
    Response { id: i64, result: Result<Value, ResponseError> },
    /// Something the server says that wants no answer.
    Notification { method: String, params: Value },
    /// Something the server asks of the client, which must be answered, with the `id` it came with.
    Request { id: Value, method: String, params: Value },
}

/// What a message body is, or why it is not a message.
pub fn parse(body: &[u8]) -> Result<Incoming, String> {
    let value: Value = serde_json::from_slice(body).map_err(|error| error.to_string())?;
    let object = value.as_object().ok_or("a message is a JSON object")?;
    let params = || object.get("params").cloned().unwrap_or(Value::Null);
    let id = object.get("id").filter(|id| !id.is_null());
    match (id, object.get("method")) {
        (Some(id), Some(method)) => Ok(Incoming::Request {
            id: id.clone(),
            method: method.as_str().ok_or("a method is a string")?.to_owned(),
            params: params(),
        }),
        (None, Some(method)) => Ok(Incoming::Notification {
            method: method.as_str().ok_or("a method is a string")?.to_owned(),
            params: params(),
        }),
        (Some(id), None) => {
            // Every request this client makes has a number for its id, so only a number answers one.
            let id = id.as_i64().ok_or("a response whose id is not one this client made")?;
            if let Some(error) = object.get("error") {
                let code = error.get("code").and_then(Value::as_i64).ok_or("an error with no code")?;
                let message = error.get("message").and_then(Value::as_str).unwrap_or_default().to_owned();
                Ok(Incoming::Response { id, result: Err(ResponseError { code, message }) })
            } else if let Some(result) = object.get("result") {
                Ok(Incoming::Response { id, result: Ok(result.clone()) })
            } else {
                Err("a response with neither a result nor an error".to_owned())
            }
        }
        (None, None) => Err("a message with neither an id nor a method".to_owned()),
    }
}

/// The body of a request.
pub fn request(id: i64, method: &str, params: Value) -> Vec<u8> {
    let mut message = json!({"jsonrpc": "2.0", "id": id, "method": method});
    with_params(&mut message, params);
    body(&message)
}

/// The body of a notification.
pub fn notification(method: &str, params: Value) -> Vec<u8> {
    let mut message = json!({"jsonrpc": "2.0", "method": method});
    with_params(&mut message, params);
    body(&message)
}

/// Puts `params` in `message`, unless there are none: a method that takes no parameters has no `params`,
/// and some servers do not take a `null` for it.
fn with_params(message: &mut Value, params: Value) {
    if !params.is_null() {
        message["params"] = params;
    }
}

/// The body of the answer to a request of the server's, with the `id` it came with.
pub fn respond(id: &Value, result: Value) -> Vec<u8> {
    body(&json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

/// The body of the answer that says the request cannot be done.
pub fn respond_error(id: &Value, code: i64, message: &str) -> Vec<u8> {
    body(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}))
}

fn body(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).expect("a JSON value can be written")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::{BufReader, Cursor, Read};

    fn framed(body: &str) -> Vec<u8> {
        let mut out = Vec::new();
        write_message(&mut out, body.as_bytes()).expect("a vector can be written to");
        out
    }

    fn read_all(bytes: Vec<u8>) -> Vec<io::Result<Option<Vec<u8>>>> {
        let mut input = Cursor::new(bytes);
        let mut out = Vec::new();
        loop {
            let next = read_message(&mut input);
            let done = !matches!(next, Ok(Some(_)));
            out.push(next);
            if done {
                return out;
            }
        }
    }

    /// A reader that gives one byte at a time, as a pipe may.
    struct Trickle(Cursor<Vec<u8>>);

    impl Read for Trickle {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let take = buf.len().min(1);
            self.0.read(&mut buf[..take])
        }
    }

    #[test]
    fn a_message_is_its_length_a_blank_line_and_its_body() {
        assert_eq!(framed("{}"), b"Content-Length: 2\r\n\r\n{}");
        // The length is of the bytes, not of the characters.
        assert_eq!(framed("\"\u{e9}\u{1f600}\""), "Content-Length: 8\r\n\r\n\"\u{e9}\u{1f600}\"".as_bytes());
        assert_eq!(framed(""), b"Content-Length: 0\r\n\r\n");
    }

    #[test]
    fn messages_that_follow_each_other_are_read_one_at_a_time_until_the_end() {
        let mut bytes = framed(r#"{"a":1}"#);
        bytes.extend(framed(r#"{"b":"é"}"#));
        bytes.extend(framed(""));
        let read = read_all(bytes);
        let bodies: Vec<Vec<u8>> = read.iter().filter_map(|read| read.as_ref().ok().cloned().flatten()).collect();
        assert_eq!(bodies, vec![br#"{"a":1}"#.to_vec(), "{\"b\":\"\u{e9}\"}".as_bytes().to_vec(), Vec::new()]);
        assert!(matches!(read.last(), Some(Ok(None))), "a closed pipe between messages is the end, not an error");
    }

    #[test]
    fn a_message_that_arrives_a_byte_at_a_time_is_read_whole() {
        let bytes = framed(r#"{"method":"x","params":[1,2,3]}"#);
        let mut input = BufReader::new(Trickle(Cursor::new(bytes)));
        assert_eq!(read_message(&mut input).unwrap().unwrap(), br#"{"method":"x","params":[1,2,3]}"#);
        assert_eq!(read_message(&mut input).unwrap(), None);
    }

    #[test]
    fn the_header_name_is_not_case_sensitive_and_other_headers_are_passed_over() {
        let bytes = b"content-length: 4\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\nnull".to_vec();
        assert_eq!(read_all(bytes)[0].as_ref().unwrap().as_deref(), Some(&b"null"[..]));
        let bytes = b"X-Other: 1\r\nCONTENT-LENGTH:   3  \r\n\r\n123".to_vec();
        assert_eq!(read_all(bytes)[0].as_ref().unwrap().as_deref(), Some(&b"123"[..]));
    }

    #[test]
    fn a_message_that_is_not_framed_as_the_protocol_says_is_an_error() {
        let invalid = |bytes: &[u8]| {
            let read = read_all(bytes.to_vec());
            assert!(matches!(&read[0], Err(error) if error.kind() == io::ErrorKind::InvalidData), "{:?}", String::from_utf8_lossy(bytes));
        };
        invalid(b"\r\n{}");
        invalid(b"Content-Type: x\r\n\r\n{}");
        invalid(b"Content-Length: abc\r\n\r\n{}");
        invalid(b"Content-Length: -1\r\n\r\n{}");
        invalid(b"Content-Length: 99999999999999999999\r\n\r\n");
        invalid(format!("Content-Length: {}\r\n\r\n", MAX_MESSAGE_BYTES + 1).as_bytes());
        invalid(b"not a header line\r\n\r\n{}");
    }

    #[test]
    fn a_pipe_that_closes_in_the_middle_of_a_message_is_an_error_and_not_a_short_message() {
        for cut in [b"Content-Length: 10\r\n\r\n{}".to_vec(), b"Content-Length: 10\r\n".to_vec(), b"Content-Len".to_vec()] {
            let read = read_all(cut);
            assert!(matches!(&read[0], Err(error) if error.kind() == io::ErrorKind::UnexpectedEof), "{read:?}");
        }
    }

    #[test]
    fn a_request_a_notification_and_the_answers_are_json_rpc() {
        let body = |bytes: Vec<u8>| serde_json::from_slice::<Value>(&bytes).expect("the body is JSON");
        assert_eq!(body(request(7, "initialize", json!({"a": 1}))), json!({"jsonrpc": "2.0", "id": 7, "method": "initialize", "params": {"a": 1}}));
        assert_eq!(body(notification("initialized", json!({}))), json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}));
        assert_eq!(body(respond(&json!("abc"), Value::Null)), json!({"jsonrpc": "2.0", "id": "abc", "result": null}));
        assert_eq!(body(respond_error(&json!(3), -32601, "no such method")), json!({"jsonrpc": "2.0", "id": 3, "error": {"code": -32601, "message": "no such method"}}));
    }

    #[test]
    fn a_message_that_has_no_parameters_has_no_params_and_not_null() {
        let body = |bytes: Vec<u8>| serde_json::from_slice::<Value>(&bytes).expect("the body is JSON");
        assert_eq!(body(request(2, "shutdown", Value::Null)), json!({"jsonrpc": "2.0", "id": 2, "method": "shutdown"}));
        assert_eq!(body(notification("exit", Value::Null)), json!({"jsonrpc": "2.0", "method": "exit"}));
    }

    #[test]
    fn what_the_server_sends_is_told_apart_by_its_id_and_its_method() {
        let parse = |text: &str| parse(text.as_bytes());
        assert_eq!(
            parse(r#"{"jsonrpc":"2.0","id":4,"result":{"ok":true}}"#),
            Ok(Incoming::Response { id: 4, result: Ok(json!({"ok": true})) })
        );
        assert_eq!(parse(r#"{"jsonrpc":"2.0","id":4,"result":null}"#), Ok(Incoming::Response { id: 4, result: Ok(Value::Null) }));
        assert_eq!(
            parse(r#"{"jsonrpc":"2.0","id":5,"error":{"code":-32803,"message":"busy","data":1}}"#),
            Ok(Incoming::Response { id: 5, result: Err(ResponseError { code: -32803, message: "busy".into() }) })
        );
        assert_eq!(
            parse(r#"{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":"x"}}"#),
            Ok(Incoming::Notification { method: "textDocument/publishDiagnostics".into(), params: json!({"uri": "x"}) })
        );
        assert_eq!(
            parse(r#"{"jsonrpc":"2.0","id":"a-1","method":"client/registerCapability","params":{}}"#),
            Ok(Incoming::Request { id: json!("a-1"), method: "client/registerCapability".into(), params: json!({}) })
        );
        // A notification may leave its params out.
        assert_eq!(parse(r#"{"jsonrpc":"2.0","method":"exit"}"#), Ok(Incoming::Notification { method: "exit".into(), params: Value::Null }));
    }

    #[test]
    fn what_is_not_a_message_is_said_so_and_does_not_panic() {
        for text in ["", "[]", "42", "not json", r#"{"jsonrpc":"2.0"}"#, r#"{"id":1}"#, r#"{"id":"x","result":1}"#, r#"{"id":1.5,"result":1}"#, r#"{"method":7}"#] {
            assert!(parse(text.as_bytes()).is_err(), "{text:?} is not a message");
        }
        assert!(parse(&[0xff, 0xfe, 0x00]).is_err(), "not text");
    }
}
