//! Detection of target-related `#[cfg(...)]` lines in Rust files and discovery of installed `rustup` targets.

use std::path::{Path, PathBuf};
use std::process::Command;

use aimer::input::input::TextSnapshot;

use crate::services::lsp::locate;

/// Maximum number of lines inspected when a `#[cfg(` attribute spans multiple lines.
const MAX_MULTILINE_CFG: usize = 16;

/// Whether `line` in `text` starts a target-related `#[cfg(...)]` or `#![cfg(...)]` attribute
/// (such as `target_os`, `target_arch`, `target_family`, `target_env`, `unix`, or `windows`).
///
/// Handles single-line and multi-line attributes while keeping the common non-attribute line
/// O(1) with zero allocations.
pub fn is_target_cfg_at(text: &TextSnapshot, line: usize) -> bool {
    if line >= text.line_count() {
        return false;
    }
    let range = text.line_range(line);
    let first = text.slice(range.clone());
    match scan_leading_cfg(&first) {
        CfgScan::FoundTarget => true,
        CfgScan::NoTarget => false,
        CfgScan::UnclosedCfg => {
            let last_line = (line + MAX_MULTILINE_CFG).min(text.line_count().saturating_sub(1));
            let end = text.line_range(last_line).end;
            let multi = text.slice(range.start..end);
            matches!(scan_leading_cfg(&multi), CfgScan::FoundTarget)
        }
    }
}

/// Whether `line` contains a leading target-related `#[cfg(...)]` or `#![cfg(...)]` attribute.
///
/// Handles optional whitespace inside the attribute (e.g. `# [ cfg ( ... ) ]`) and multiple
/// attributes on the same line (e.g. `#[inline] #[cfg(unix)]`). Ignores comments, string
/// literals, `#[cfg_attr(...)]`, and non-target predicates like `feature` or `test`.
pub fn is_cfg_line(line: &str) -> bool {
    matches!(scan_leading_cfg(line), CfgScan::FoundTarget)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CfgScan {
    FoundTarget,
    NoTarget,
    UnclosedCfg,
}

fn scan_leading_cfg(src: &str) -> CfgScan {
    let bytes = src.as_bytes();
    let mut i = skip_ascii_ws(bytes, 0);
    // Scan leading attributes on the line; stop as soon as non-attribute tokens appear.
    while i < bytes.len() && bytes[i] == b'#' {
        i += 1;
        i = skip_ascii_ws(bytes, i);
        if i < bytes.len() && bytes[i] == b'!' {
            i += 1;
            i = skip_ascii_ws(bytes, i);
        }
        if i >= bytes.len() || bytes[i] != b'[' {
            return CfgScan::NoTarget;
        }
        i += 1;
        let name_start = skip_ascii_ws(bytes, i);
        if bytes[name_start..].starts_with(b"cfg") {
            let after_cfg = skip_ascii_ws(bytes, name_start + 3);
            if after_cfg < bytes.len() && bytes[after_cfg] == b'(' {
                match scan_cfg_parens(bytes, after_cfg + 1) {
                    ParenScan::FoundTarget => return CfgScan::FoundTarget,
                    ParenScan::Unclosed => return CfgScan::UnclosedCfg,
                    ParenScan::ClosedNoTarget(_) => {}
                }
            }
        }
        // Advance past the matching `]` to check any following attribute on the same line.
        let Some(end) = skip_bracket_body(bytes, name_start) else {
            return CfgScan::NoTarget;
        };
        i = skip_ascii_ws(bytes, end);
    }
    CfgScan::NoTarget
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParenScan {
    FoundTarget,
    ClosedNoTarget(usize),
    Unclosed,
}

/// Whether `ident` is a target-related `cfg` predicate (`unix`, `windows`, or `target_*`).
fn is_target_predicate(ident: &[u8]) -> bool {
    ident == b"unix" || ident == b"windows" || ident.starts_with(b"target_")
}

/// Scans inside `cfg(` up to its matching `)`, ignoring string literals and comments, to check
/// whether any identifier predicate is target-related.
fn scan_cfg_parens(bytes: &[u8], mut i: usize) -> ParenScan {
    let mut depth = 1usize;
    let mut in_string = false;
    let mut escaped = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        // Skip `//` line comments and `/* ... */` block comments.
        if b == b'/' && i + 1 < bytes.len() {
            if bytes[i + 1] == b'/' {
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if bytes[i + 1] == b'*' {
                i += 2;
                let mut comment_depth = 1usize;
                while i + 1 < bytes.len() && comment_depth > 0 {
                    if bytes[i] == b'/' && bytes[i + 1] == b'*' {
                        comment_depth += 1;
                        i += 2;
                    } else if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        comment_depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                continue;
            }
        }
        match b {
            b'"' => {
                in_string = true;
                i += 1;
            }
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    return ParenScan::ClosedNoTarget(i);
                }
            }
            b if b.is_ascii_alphabetic() || b == b'_' => {
                let start = i;
                i += 1;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                if is_target_predicate(&bytes[start..i]) {
                    return ParenScan::FoundTarget;
                }
            }
            _ => {
                i += 1;
            }
        }
    }
    ParenScan::Unclosed
}

/// Skips ASCII whitespace starting at `i` and returns the next index.
fn skip_ascii_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// Advances from inside `[` to the index immediately after its matching `]`, respecting nested
/// brackets and double-quoted string literals. Returns `None` if unclosed on this line.
fn skip_bracket_body(bytes: &[u8], mut i: usize) -> Option<usize> {
    let mut depth = 1usize;
    let mut in_string = false;
    let mut escaped = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
        } else {
            match b {
                b'"' => in_string = true,
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i + 1);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// Lists the target triples installed in the active `rustup` toolchain for `root`.
///
/// Returns an empty list if `rustup` is not installed or fails to run.
pub fn installed_targets(root: &Path) -> Vec<String> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let program = locate::resolve("rustup", std::env::var_os("PATH"), home.as_deref());
    let mut command = Command::new(&program);
    command.args(["target", "list", "--installed"]);
    if root.is_dir() {
        command.current_dir(root);
    }
    let Ok(output) = command.output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_installed_targets(&stdout)
}

/// Returns the default host target triple reported by `rustc -vV` for `root`, if available.
pub fn host_target(root: &Path) -> Option<String> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let program = locate::resolve("rustc", std::env::var_os("PATH"), home.as_deref());
    let mut command = Command::new(&program);
    command.arg("-vV");
    if root.is_dir() {
        command.current_dir(root);
    }
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_host_target(&stdout)
}

/// Parses the output of `rustup target list --installed` into a list of target triples.
pub fn parse_installed_targets(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Parses the `host: <triple>` line from `rustc -vV` output.
pub fn parse_host_target(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        let triple = line.trim().strip_prefix("host:")?.trim();
        (!triple.is_empty()).then(|| triple.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_target_related_outer_and_inner_cfg_attributes() {
        assert!(is_cfg_line("#[cfg(unix)]"));
        assert!(is_cfg_line("#[cfg(windows)]"));
        assert!(is_cfg_line("#![cfg(not(target_os = \"windows\"))]"));
        assert!(is_cfg_line(
            "\t#[cfg(all(target_os = \"macos\", target_arch = \"aarch64\"))]"
        ));
        assert!(is_cfg_line("    #[cfg(target_family = \"unix\")]"));
        assert!(is_cfg_line("#[cfg(target_env = \"msvc\")]"));
        assert!(is_cfg_line("#[cfg(target_abi = \"sim\")]"));
        assert!(is_cfg_line("#[cfg(target_pointer_width = \"64\")]"));
        assert!(is_cfg_line("#[cfg(target_endian = \"little\")]"));
        assert!(is_cfg_line("#[cfg(target_vendor = \"apple\")]"));
        assert!(is_cfg_line("#[cfg(target_feature = \"neon\")]"));
        assert!(is_cfg_line("#[cfg(all(windows, feature = \"win-system-proxy\"))]"));
        assert!(is_cfg_line("# [ cfg ( unix ) ]"));
        assert!(is_cfg_line("# ! [ cfg ( target_os = \"linux\" ) ]"));
        assert!(is_cfg_line("#[cfg(unix)] pub fn only_unix() {}"));
        assert!(is_cfg_line("#[inline] #[cfg(target_os = \"linux\")]"));
        assert!(is_cfg_line("#[cfg(feature = \"a\")] #[cfg(unix)]"));
        assert!(is_cfg_line("#[doc = \"[cfg]\"] #[cfg(unix)]"));
    }

    #[test]
    fn ignores_non_target_cfg_attributes() {
        assert!(!is_cfg_line("#[cfg(test)]"));
        assert!(!is_cfg_line("    #[cfg(feature = \"cookies\")]"));
        assert!(!is_cfg_line("#[cfg(feature = \"windows\")]"));
        assert!(!is_cfg_line("#[cfg(feature = \"target_os\")]"));
        assert!(!is_cfg_line("#[cfg(all(test, feature = \"_test\"))]"));
        assert!(!is_cfg_line("#[cfg(debug_assertions)]"));
        assert!(!is_cfg_line("#[cfg(doc)]"));
        assert!(!is_cfg_line("#[cfg(/* target_os */ feature = \"x\")]"));
    }

    #[test]
    fn detects_multiline_target_cfg_only_on_its_first_line() {
        let src = "#[cfg(\n    any(\n        target_os = \"macos\",\n        target_os = \"ios\",\n    )\n)]\nfn f() {}\n";
        let text = aimer::input::input::TextSnapshot::from_text(src);
        assert!(is_target_cfg_at(&text, 0));
        assert!(!is_target_cfg_at(&text, 1));
        assert!(!is_target_cfg_at(&text, 2));
        assert!(!is_target_cfg_at(&text, 6));

        let non_target = "#[cfg(\n    all(\n        test,\n        feature = \"cookies\",\n    )\n)]\n";
        let non_target_text = aimer::input::input::TextSnapshot::from_text(non_target);
        assert!(!is_target_cfg_at(&non_target_text, 0));
    }

    #[test]
    fn ignores_comments_strings_cfg_attr_and_other_code() {
        assert!(!is_cfg_line(""));
        assert!(!is_cfg_line("fn main() {}"));
        assert!(!is_cfg_line("#[derive(Debug)]"));
        assert!(!is_cfg_line("#[cfg_attr(unix, inline)]"));
        assert!(!is_cfg_line("// #[cfg(unix)]"));
        assert!(!is_cfg_line("/// #[cfg(feature = \"json\")]"));
        assert!(!is_cfg_line("/* #[cfg(unix)] */"));
        assert!(!is_cfg_line("let s = \"#[cfg(unix)]\";"));
        assert!(!is_cfg_line("#[doc = \"unclosed"));
    }

    #[test]
    fn parses_installed_targets_with_blank_lines_and_crlf() {
        let out = "aarch64-apple-darwin\r\n\n  x86_64-pc-windows-msvc  \nwasm32-unknown-unknown\r\n";
        assert_eq!(
            parse_installed_targets(out),
            vec![
                "aarch64-apple-darwin",
                "x86_64-pc-windows-msvc",
                "wasm32-unknown-unknown",
            ]
        );
        assert!(parse_installed_targets("  \n\r\n").is_empty());
    }

    #[test]
    fn parses_host_target_from_rustc_vv() {
        let out = "rustc 1.85.0 (4d91de4e4 2025-02-17)\n\
                   binary: rustc\n\
                   commit-hash: 4d91de4e4\n\
                   host: aarch64-apple-darwin\n\
                   release: 1.85.0\n";
        assert_eq!(
            parse_host_target(out).as_deref(),
            Some("aarch64-apple-darwin")
        );
        assert_eq!(parse_host_target("rustc 1.85.0\n"), None);
    }
}
