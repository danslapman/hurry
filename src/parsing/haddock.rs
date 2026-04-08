use tree_sitter::Node;

use crate::parsing::haskell::{COMMENT_KINDS, SIGNATURE};
use crate::symbols::index::WorkspaceIndex;

// ── Haddock extraction ────────────────────────────────────────────────────────

/// Extract the Haddock comment for `node`.
///
/// Looks for a forward-doc comment (`-- |` / `{- |`) immediately preceding
/// the node, skipping over any `signature` node that sits between the haddock
/// and the declaration.  If no haddock is found among the node's own siblings
/// (which happens for the first declaration in a module, whose haddock lives
/// at the root level as a sibling of the whole `declarations` block), we walk
/// up to the parent and check there.  Also checks for a backward-doc (`-- ^`)
/// comment immediately following the node.
pub fn extract_haddock(node: Node<'_>, source: &str) -> Option<String> {
    // 1. Forward Haddock: check direct preceding siblings (skipping signatures)
    let (result, reached_start) = preceding_haddock_inner(node, source);
    if result.is_some() {
        return result;
    }

    // If we exhausted all preceding siblings without finding a non-haddock
    // node, the haddock may live at the parent level (e.g. first declaration
    // in a module whose `-- |` comment precedes the `declarations` block).
    if reached_start {
        if let Some(parent) = node.parent() {
            let (parent_result, _) = preceding_haddock_inner(parent, source);
            if parent_result.is_some() {
                return parent_result;
            }
        }
    }

    // 2. Backward Haddock: `-- ^` immediately after the node
    if let Some(next) = node.next_named_sibling() {
        if COMMENT_KINDS.contains(&next.kind()) {
            let raw = &source[next.byte_range()];
            if raw.trim_start().starts_with("-- ^") {
                let text = strip_haddock_line(raw);
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }
    }

    None
}

/// Walk backwards from `node` looking for a forward Haddock comment,
/// skipping `signature` nodes (which appear between the haddock and the
/// function/data declaration in Haskell source).
///
/// Collects **all** consecutive Haddock comment nodes so that multi-line docs
/// written as separate `-- |` line comments are combined correctly.  This is
/// important on Windows, where CRLF line endings cause tree-sitter to emit one
/// comment node per line rather than a single multi-line node.
///
/// Returns `(found_doc, reached_start)` where `reached_start` is `true` when
/// all preceding siblings were exhausted without encountering a non-signature,
/// non-comment node — indicating this is the first declaration in its scope
/// and the haddock might be at a higher level in the tree.
fn preceding_haddock_inner(node: Node<'_>, source: &str) -> (Option<String>, bool) {
    let mut sib = node.prev_named_sibling();
    let mut collected: Vec<String> = Vec::new();
    let mut reached_start = false;

    loop {
        match sib {
            None => {
                reached_start = true;
                break;
            }
            Some(s) => {
                let kind = s.kind();
                // Signatures sit between the haddock and the declaration; skip them.
                if kind == SIGNATURE {
                    sib = s.prev_named_sibling();
                    continue;
                }
                if COMMENT_KINDS.contains(&kind) {
                    let raw = &source[s.byte_range()];
                    let trimmed = raw.trim();
                    if trimmed.starts_with("-- |")
                        || trimmed.starts_with("{-|")
                        || trimmed.starts_with("{- |")
                    {
                        let text = strip_haddock_line(raw);
                        if !text.is_empty() {
                            collected.push(text);
                        }
                        sib = s.prev_named_sibling();
                        continue;
                    }
                    // A non-forward comment (e.g. plain `--`) — stop here; the
                    // parent heuristic does not apply.
                    break;
                }
                // Any other sibling type — stop.
                break;
            }
        }
    }

    if !collected.is_empty() {
        collected.reverse();
        return (Some(collected.join("\n")), false);
    }

    (None, reached_start)
}

fn strip_haddock_line(raw: &str) -> String {
    raw.lines()
        .map(|line| {
            let t = line.trim();
            // Block comment delimiters
            if t == "{-" || t == "-}" {
                return "";
            }
            // Strip opening {- | or {-|
            let t = t
                .strip_prefix("{- |")
                .or_else(|| t.strip_prefix("{-|"))
                .unwrap_or(t);
            // Strip closing -}
            let t = t.strip_suffix("-}").unwrap_or(t);
            // Strip -- | (haddock forward marker)
            let t = t
                .strip_prefix("-- |")
                .or_else(|| t.strip_prefix("-- ^"))
                .or_else(|| t.strip_prefix("--"))
                .unwrap_or(t);
            t.trim()
        })
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

// ── Haddock → Markdown rendering ─────────────────────────────────────────────

/// Render a Haddock documentation string to Markdown for LSP hover display.
pub fn haddock_to_markdown(text: &str, index: &WorkspaceIndex) -> String {
    let text = render_code_blocks(text);
    let text = render_bird_tracks(&text);
    let text = render_inline_markup(&text, index);
    text
}

/// Replace multiline `@...@` with fenced haskell code blocks.
fn render_code_blocks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('@') {
        out.push_str(&rest[..start]);
        rest = &rest[start + 1..];
        if let Some(end) = rest.find('@') {
            let code = &rest[..end];
            if code.contains('\n') {
                out.push_str("```haskell\n");
                out.push_str(code.trim());
                out.push_str("\n```");
            } else {
                out.push('`');
                out.push_str(code);
                out.push('`');
            }
            rest = &rest[end + 1..];
        } else {
            out.push('@');
        }
    }
    out.push_str(rest);
    out
}

/// Replace `> line` bird-track style code with fenced haskell code block.
fn render_bird_tracks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_block = false;

    for line in text.lines() {
        if let Some(stripped) = line.strip_prefix("> ").or_else(|| line.strip_prefix(">")) {
            if !in_block {
                out.push_str("```haskell\n");
                in_block = true;
            }
            out.push_str(stripped);
            out.push('\n');
        } else {
            if in_block {
                out.push_str("```\n");
                in_block = false;
            }
            out.push_str(line);
            out.push('\n');
        }
    }
    if in_block {
        out.push_str("```\n");
    }
    out.trim_end().to_string()
}

/// Replace inline Haddock markup with Markdown equivalents.
fn render_inline_markup(text: &str, index: &WorkspaceIndex) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        // /italic/
        if chars[i] == '/' && i + 1 < chars.len() && chars[i + 1] != ' ' {
            if let Some(end) = find_closing(&chars, i + 1, '/') {
                let inner: String = chars[i + 1..end].iter().collect();
                out.push('*');
                out.push_str(&inner);
                out.push('*');
                i = end + 1;
                continue;
            }
        }
        // __bold__
        if chars[i] == '_'
            && i + 1 < chars.len()
            && chars[i + 1] == '_'
            && i + 2 < chars.len()
            && chars[i + 2] != ' '
        {
            let rest: String = chars[i + 2..].iter().collect();
            if let Some(end_offset) = rest.find("__") {
                let inner = &rest[..end_offset];
                out.push_str("**");
                out.push_str(inner);
                out.push_str("**");
                i += 2 + end_offset + 2;
                continue;
            }
        }
        // 'identifier' → `identifier` or link
        if chars[i] == '\''
            && i + 1 < chars.len()
            && chars[i + 1] != ' '
            && chars[i + 1] != '\''
        {
            if let Some(end) = find_closing(&chars, i + 1, '\'') {
                let name: String = chars[i + 1..end].iter().collect();
                if !name.is_empty() && !name.contains(' ') {
                    let matches = index.lookup_by_name(&name);
                    if matches.len() == 1 {
                        out.push('[');
                        out.push_str(&name);
                        out.push_str("](");
                        out.push_str(matches[0].uri.as_str());
                        out.push(')');
                    } else {
                        out.push('`');
                        out.push_str(&name);
                        out.push('`');
                    }
                    i = end + 1;
                    continue;
                }
            }
        }
        // <url> → [url](url)
        if chars[i] == '<' {
            if let Some(end) = find_closing(&chars, i + 1, '>') {
                let url: String = chars[i + 1..end].iter().collect();
                if url.starts_with("http://") || url.starts_with("https://") {
                    out.push('[');
                    out.push_str(&url);
                    out.push_str("](");
                    out.push_str(&url);
                    out.push(')');
                    i = end + 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn find_closing(chars: &[char], from: usize, close: char) -> Option<usize> {
    for j in from..chars.len() {
        if chars[j] == close {
            return Some(j);
        }
        if chars[j] == '\n' {
            // Don't cross line boundaries for inline markup
            return None;
        }
    }
    None
}
