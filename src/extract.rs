// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Minimal article fetch + HTML-to-text for `--url` mode. Deliberately dependency-free:
//! we only need the gist, which then feeds the scriptwriter.

use anyhow::{Context, Result};

pub async fn fetch_article(url: &str) -> Result<String> {
    let html = reqwest::get(url)
        .await
        .with_context(|| format!("failed to fetch {url}"))?
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
    let lower = input.to_lowercase();
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = String::new();
    let mut i = 0;
    while i < input.len() {
        if let Some(rel) = lower[i..].find(&open) {
            let start = i + rel;
            out.push_str(&input[i..start]);
            match lower[start..].find(&close) {
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
