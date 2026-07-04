// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Minimal article fetch + HTML-to-text for `--url` mode. Deliberately dependency-free:
//! we only need the gist, which then feeds the scriptwriter.

use std::net::{IpAddr, ToSocketAddrs};

use anyhow::{bail, Context, Result};
use futures::StreamExt;
use reqwest::{header, Url};

const MAX_ARTICLE_BYTES: usize = 2 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;

/// Fetch `url` and return its main text content as a single, whitespace-collapsed string.
///
/// This is the entry point for `--url` mode: the returned text is handed to the scriptwriter
/// (`script::from_article`), so it only needs to be "good enough" prose — we deliberately skip
/// a real HTML parser and just strip tags (see [`html_to_text`]). Errors are surfaced with
/// context for the network fetch, a non-2xx status, and body decoding.
pub async fn fetch_article(url: &str) -> Result<String> {
    // Many sites (e.g. Wikipedia, per Wikimedia's User-Agent policy) reject
    // requests without a browser-like User-Agent with 403, so set one. The UA is built from
    // Cargo package metadata at compile time so it stays accurate across version bumps.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION"),
            " (https://github.com/spunkytensor/reel-maestro)"
        ))
        .build()
        .context("failed to build HTTP client")?;
    let mut current = Url::parse(url).with_context(|| format!("invalid article URL {url:?}"))?;
    let mut resp = None;
    for _ in 0..=MAX_REDIRECTS {
        validate_fetch_url(&current)?;
        let r = client
            .get(current.clone())
            .send()
            .await
            .with_context(|| format!("failed to fetch {current}"))?;
        if r.status().is_redirection() {
            let loc = r
                .headers()
                .get(header::LOCATION)
                .ok_or_else(|| anyhow::anyhow!("redirect from {current} did not include Location"))?
                .to_str()
                .with_context(|| format!("redirect Location from {current} is not valid text"))?;
            current = current
                .join(loc)
                .with_context(|| format!("invalid redirect target {loc:?} from {current}"))?;
            continue;
        }
        resp = Some(
            r.error_for_status()
                .with_context(|| format!("server returned an error for {current}"))?,
        );
        break;
    }
    let resp = resp.ok_or_else(|| anyhow::anyhow!("too many redirects while fetching {url}"))?;
    if let Some(len) = resp.content_length() {
        if len > MAX_ARTICLE_BYTES as u64 {
            bail!("article response is too large ({len} bytes; max {MAX_ARTICLE_BYTES})");
        }
    }
    let mut body = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("failed to read article response body")?;
        if body.len() + chunk.len() > MAX_ARTICLE_BYTES {
            bail!("article response exceeded {MAX_ARTICLE_BYTES} bytes");
        }
        body.extend_from_slice(&chunk);
    }
    let html = String::from_utf8(body).context("article response was not valid UTF-8")?;
    Ok(html_to_text(&html))
}

fn validate_fetch_url(url: &Url) -> Result<()> {
    match url.scheme() {
        "http" | "https" => {}
        other => bail!("unsupported article URL scheme {other:?}; use http or https"),
    }
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("article URL has no host: {url}"))?;
    if host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost") {
        bail!("article URL must not target localhost: {url}");
    }
    let port = url.port_or_known_default().unwrap_or(443);
    let addrs = (host, port)
        .to_socket_addrs()
        .with_context(|| format!("could not resolve article URL host {host:?}"))?;
    let ips: Vec<IpAddr> = addrs.map(|a| a.ip()).collect();
    if ips.is_empty() {
        bail!("article URL host resolved to no addresses: {host}");
    }
    if let Some(ip) = ips.into_iter().find(|ip| !is_public_ip(*ip)) {
        bail!("article URL must not resolve to private or local address {ip}: {url}");
    }
    Ok(())
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified())
        }
        IpAddr::V6(ip) => {
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local())
        }
    }
}

/// Crudely convert an HTML document to plain text.
///
/// This is intentionally a heuristic, not a parser: we (1) delete `<script>`/`<style>` blocks
/// whose *contents* would otherwise leak into the text, (2) drop everything between `<` and `>`
/// to strip the remaining tags, and (3) collapse runs of whitespace and truncate. The result is
/// noisy (no entity decoding, nav/boilerplate kept) but the downstream LLM tolerates that, and
/// avoiding an HTML-parser dependency keeps the binary small.
fn html_to_text(html: &str) -> String {
    // Remove tag *bodies* first — a plain tag strip would keep the JS/CSS source as text.
    let without_scripts = remove_blocks(html, "script");
    let cleaned = remove_blocks(&without_scripts, "style");

    // Strip the remaining tags by skipping any character between `<` and `>`.
    let mut out = String::new();
    let mut in_tag = false;
    for c in cleaned.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c), // outside a tag → keep visible text
            _ => {}                      // inside a tag → drop
        }
    }

    // Collapse whitespace (HTML is full of newlines/indentation) and cap length so the prompt
    // stays cheap; 12k chars is plenty of gist for the scriptwriter.
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(12_000).collect()
}

/// Remove `<tag ...> ... </tag>` blocks (case-insensitive), used to drop scripts/styles.
///
/// Walks `input` left to right, copying text up to each opening tag and skipping everything
/// through the matching close tag. `open` is matched as `<tag` (no `>`) so it catches tags with
/// attributes like `<script src=...>`. An unterminated block (open with no close) drops the
/// remainder of the document, which is the safe choice for stripping unwanted content.
fn remove_blocks(input: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = String::new();
    let mut i = 0; // byte cursor into `input`
    while i < input.len() {
        // `rel` is relative to the `i..` slice, so add `i` back to get an absolute offset.
        if let Some(rel) = find_case_insensitive(&input[i..], &open) {
            let start = i + rel;
            out.push_str(&input[i..start]); // keep text before the opening tag
            match find_case_insensitive(&input[start..], &close) {
                // Jump the cursor past the close tag, discarding the block in between.
                Some(end_rel) => i = start + end_rel + close.len(),
                None => break, // unterminated; drop the rest
            }
        } else {
            out.push_str(&input[i..]); // no more blocks — copy the tail verbatim
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
    // Pre-fold the needle to lowercase ASCII once, so the inner loop only folds the haystack.
    let needle_lower: Vec<u8> = needle.bytes().map(|b| b.to_ascii_lowercase()).collect();
    if needle_lower.is_empty() {
        return Some(0); // an empty needle matches at the start, mirroring `str::find`
    }
    // Only try positions that begin a UTF-8 character, so the returned index is always a valid
    // char boundary (safe to slice at). At each candidate, compare the next N folded bytes.
    haystack.char_indices().map(|(idx, _)| idx).find(|&idx| {
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
