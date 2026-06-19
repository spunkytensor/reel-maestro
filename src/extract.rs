// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Minimal article fetch + HTML-to-text for `--url` mode. Deliberately dependency-free:
//! we only need the gist, which then feeds the scriptwriter.

use anyhow::{Context, Result};

pub async fn fetch_article(url: &str) -> Result<String> {
    // Many sites (e.g. Wikipedia, per Wikimedia's User-Agent policy) reject
    // requests without a browser-like User-Agent with 403, so set one.
    let client = reqwest::Client::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION"),
            " (https://github.com/spunkytensor/reel-maestro)"
        ))
        .build()
        .context("failed to build HTTP client")?;
    let html = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed to fetch {url}"))?
        .error_for_status()
        .with_context(|| format!("server returned an error for {url}"))?
        .text()
        .await?;
    Ok(html_to_text(&html))
}

fn html_to_text(html: &str) -> String {
    let without_scripts = remove_blocks(html, "script");
    let cleaned = remove_blocks(&without_scripts, "style");

    let mut out = String::new();
    let mut in_tag = false;
    for c in cleaned.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }

    // Collapse whitespace and cap length so the prompt stays cheap.
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(12_000).collect()
}

/// Remove `<tag ...> ... </tag>` blocks (case-insensitive), used to drop scripts/styles.
fn remove_blocks(input: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = String::new();
    let mut i = 0;
    while i < input.len() {
        if let Some(rel) = find_case_insensitive(&input[i..], &open) {
            let start = i + rel;
            out.push_str(&input[i..start]);
            match find_case_insensitive(&input[start..], &close) {
                Some(end_rel) => i = start + end_rel + close.len(),
                None => break, // unterminated; drop the rest
            }
        } else {
            out.push_str(&input[i..]);
            break;
        }
    }
    out
}

/// ASCII-case-insensitive substring search returning a byte index into
/// `haystack`. Operates on `haystack` directly (no lowercased copy) so the
/// returned offset is always a valid char boundary of `haystack`. Tag names
/// are ASCII, so ASCII case folding is sufficient and avoids the byte-length
/// drift that full Unicode `to_lowercase()` can introduce.
fn find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let needle_lower: Vec<u8> = needle.bytes().map(|b| b.to_ascii_lowercase()).collect();
    if needle_lower.is_empty() {
        return Some(0);
    }
    haystack
        .char_indices()
        .map(|(idx, _)| idx)
        .find(|&idx| {
            haystack[idx..]
                .bytes()
                .map(|b| b.to_ascii_lowercase())
                .take(needle_lower.len())
                .eq(needle_lower.iter().copied())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_block_case_insensitively() {
        let input = "a<SCRIPT>drop me</Script>b";
        assert_eq!(remove_blocks(input, "script"), "ab");
    }

    #[test]
    fn keeps_unterminated_open_tag_dropped() {
        // Unterminated block: everything from the open tag is dropped.
        let input = "keep<script>no close";
        assert_eq!(remove_blocks(input, "script"), "keep");
    }

    #[test]
    fn handles_length_changing_unicode_before_tag() {
        // `İ` (U+0130, 2 bytes) lowercases to 2 bytes under full Unicode
        // folding ("i" + combining dot), so byte offsets from a lowercased
        // copy would drift and could panic. Verify we slice safely.
        let input = "İİİ<script>x</script>tail İ";
        assert_eq!(remove_blocks(input, "script"), "İİİtail İ");
    }

    #[test]
    fn passes_through_when_no_tag() {
        let input = "plain İ text with no blocks";
        assert_eq!(remove_blocks(input, "script"), input);
    }
}
