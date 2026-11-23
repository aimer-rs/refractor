//! Signature help: the parameters of the function call the caret is inside (`textDocument/signatureHelp`), and which
//! parameter is currently active.

use std::path::Path;

use serde_json::{Value, json};

use super::super::position::{Encoding, Position};
use super::uri_of;

/// The method that asks for the signature help at a position in a file.
pub const SIGNATURE_HELP: &str = "textDocument/signatureHelp";

/// The parameters of the active signature at a call site, and which one the caret is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureHelp {
    pub parameters: Vec<String>,
    /// Index into [`parameters`](Self::parameters) of the active parameter, or `None` when none is active
    /// (for example when the caret is past the last parameter).
    pub active_parameter: Option<usize>,
}

/// What the client says of signature help in `initialize`.
pub fn client_capability() -> Value {
    json!({
        "dynamicRegistration": false,
        "signatureInformation": {
            "parameterInformation": {
                "labelOffsetSupport": true,
            },
            "activeParameterSupport": true,
        },
        "contextSupport": false,
    })
}

/// The parameters of `textDocument/signatureHelp` at `at` in the file `path`.
pub fn signature_params(path: &Path, at: Position) -> Value {
    json!({
        "textDocument": {"uri": uri_of(path)},
        "position": {"line": at.line, "character": at.character},
    })
}

/// Reads the active signature's parameters from the server's answer to `textDocument/signatureHelp`.
/// Returns `None` when the answer is `null`, malformed, has no signatures, or the active signature has no parameters.
pub fn signature_help(result: &Value, encoding: Encoding) -> Option<SignatureHelp> {
    let signatures = result.get("signatures")?.as_array()?;
    if signatures.is_empty() {
        return None;
    }
    let active_sig = result
        .get("activeSignature")
        .and_then(Value::as_u64)
        .and_then(|index| usize::try_from(index).ok())
        .unwrap_or(0);
    let signature = signatures.get(active_sig).or_else(|| signatures.first())?;
    let sig_label = signature.get("label")?.as_str()?;
    let raw_params = signature.get("parameters")?.as_array()?;
    let parameters: Vec<String> = raw_params
        .iter()
        .filter_map(|param| parameter_label(param.get("label")?, sig_label, encoding))
        .collect();
    if parameters.is_empty() {
        return None;
    }
    // Signature-level activeParameter overrides top-level activeParameter (LSP 3.16+).
    let raw_active = signature
        .get("activeParameter")
        .and_then(Value::as_u64)
        .or_else(|| result.get("activeParameter").and_then(Value::as_u64));
    let active_parameter = match raw_active {
        Some(index) => usize::try_from(index)
            .ok()
            .filter(|&index| index < parameters.len()),
        None => Some(0),
    };
    Some(SignatureHelp {
        parameters,
        active_parameter,
    })
}

/// Extracts one parameter's label: either a literal string or a `[start, end]` offset slice into `signature_label`.
fn parameter_label(label: &Value, signature_label: &str, encoding: Encoding) -> Option<String> {
    let text = match label {
        Value::String(text) => text.trim().to_owned(),
        Value::Array(pair) if pair.len() == 2 => {
            let start = usize::try_from(pair[0].as_u64()?).ok()?;
            let end = usize::try_from(pair[1].as_u64()?).ok()?;
            let (start_byte, end_byte) = match encoding {
                Encoding::Utf8 => (
                    clamp_char_boundary(signature_label, start),
                    clamp_char_boundary(signature_label, end),
                ),
                Encoding::Utf16 => (
                    utf16_offset_to_byte(signature_label, start),
                    utf16_offset_to_byte(signature_label, end),
                ),
            };
            if start_byte >= end_byte {
                return None;
            }
            signature_label[start_byte..end_byte].trim().to_owned()
        }
        _ => return None,
    };
    (!text.is_empty()).then_some(text)
}

/// Clamps `byte` to a valid UTF-8 character boundary in `text`, moving left if it lands inside a multi-byte character.
fn clamp_char_boundary(text: &str, byte: usize) -> usize {
    let mut index = byte.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// Converts a UTF-16 code-unit offset in `text` to a byte offset.
fn utf16_offset_to_byte(text: &str, units: usize) -> usize {
    let mut seen = 0;
    for (byte, ch) in text.char_indices() {
        if seen >= units {
            return byte;
        }
        seen += ch.len_utf16();
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn the_request_names_the_file_and_the_position() {
        assert_eq!(SIGNATURE_HELP, "textDocument/signatureHelp");
        assert_eq!(
            signature_params(
                &PathBuf::from("/p/src/main.rs"),
                Position {
                    line: 4,
                    character: 12,
                }
            ),
            json!({
                "textDocument": {"uri": "file:///p/src/main.rs"},
                "position": {"line": 4, "character": 12},
            })
        );
    }

    #[test]
    fn string_parameter_labels_and_active_parameter_are_parsed() {
        let answer = json!({
            "signatures": [{
                "label": "fn connect(host: &str, port: u16, tls: bool)",
                "parameters": [
                    {"label": "host: &str"},
                    {"label": "port: u16"},
                    {"label": "tls: bool"},
                ],
            }],
            "activeSignature": 0,
            "activeParameter": 1,
        });
        assert_eq!(
            signature_help(&answer, Encoding::Utf8),
            Some(SignatureHelp {
                parameters: vec![
                    "host: &str".to_owned(),
                    "port: u16".to_owned(),
                    "tls: bool".to_owned(),
                ],
                active_parameter: Some(1),
            })
        );
    }

    #[test]
    fn offset_parameter_labels_respect_utf8_and_utf16_encoding() {
        // `fn f(é: i32, b: u8)`: `é` is 2 bytes in UTF-8, 1 code unit in UTF-16.
        let label = "fn f(\u{e9}: i32, b: u8)";
        // UTF-8: `é: i32` is bytes 5..12, `b: u8` is bytes 14..19.
        let utf8_answer = json!({
            "signatures": [{
                "label": label,
                "parameters": [
                    {"label": [5, 12]},
                    {"label": [14, 19]},
                ],
                "activeParameter": 0,
            }],
        });
        assert_eq!(
            signature_help(&utf8_answer, Encoding::Utf8),
            Some(SignatureHelp {
                parameters: vec!["\u{e9}: i32".to_owned(), "b: u8".to_owned()],
                active_parameter: Some(0),
            })
        );

        // UTF-16: `é: i32` is units 5..11, `b: u8` is units 13..18.
        let utf16_answer = json!({
            "signatures": [{
                "label": label,
                "parameters": [
                    {"label": [5, 11]},
                    {"label": [13, 18]},
                ],
                "activeParameter": 1,
            }],
        });
        assert_eq!(
            signature_help(&utf16_answer, Encoding::Utf16),
            Some(SignatureHelp {
                parameters: vec!["\u{e9}: i32".to_owned(), "b: u8".to_owned()],
                active_parameter: Some(1),
            })
        );
    }

    #[test]
    fn signature_active_parameter_overrides_top_level_and_out_of_range_is_none() {
        let answer = json!({
            "signatures": [{
                "label": "fn from(value: T) -> Self",
                "parameters": [{"label": "value: T"}],
                "activeParameter": 0,
            }],
            "activeParameter": 1,
        });
        assert_eq!(
            signature_help(&answer, Encoding::Utf8)
                .unwrap()
                .active_parameter,
            Some(0),
            "signature-level activeParameter takes precedence"
        );

        let too_many = json!({
            "signatures": [{
                "label": "fn from(value: T) -> Self",
                "parameters": [{"label": "value: T"}],
            }],
            "activeParameter": 2,
        });
        assert_eq!(
            signature_help(&too_many, Encoding::Utf8)
                .unwrap()
                .active_parameter,
            None,
            "past the last parameter means none is active"
        );

        let omitted = json!({
            "signatures": [{
                "label": "fn from(value: T) -> Self",
                "parameters": [{"label": "value: T"}],
            }],
        });
        assert_eq!(
            signature_help(&omitted, Encoding::Utf8)
                .unwrap()
                .active_parameter,
            Some(0),
            "omitted activeParameter defaults to the first"
        );
    }

    #[test]
    fn null_empty_and_zero_parameter_signatures_are_none() {
        assert_eq!(signature_help(&Value::Null, Encoding::Utf8), None);
        assert_eq!(signature_help(&json!({"signatures": []}), Encoding::Utf8), None);
        assert_eq!(
            signature_help(
                &json!({"signatures": [{"label": "fn noop()", "parameters": []}]}),
                Encoding::Utf8
            ),
            None
        );
        assert_eq!(
            signature_help(
                &json!({"signatures": [{"label": "fn noop()"}]}),
                Encoding::Utf8
            ),
            None
        );
    }

    #[test]
    fn initialize_advertises_signature_help_capability_and_reads_server_support() {
        let params = super::super::initialize_params(Path::new("/p"), 1);
        assert_eq!(
            params["capabilities"]["textDocument"]["signatureHelp"],
            client_capability()
        );
        serde_json::from_value::<lsp_types::InitializeParams>(params)
            .expect("valid LSP InitializeParams");

        let cap = |capabilities: Value| {
            super::super::initialize_result(&json!({"capabilities": capabilities}))
                .unwrap()
                .signature_help
        };
        assert!(cap(json!({"signatureHelpProvider": {"triggerCharacters": ["(", ","]}})));
        assert!(cap(json!({"signatureHelpProvider": {}})));
        assert!(!cap(json!({})));
    }
}
