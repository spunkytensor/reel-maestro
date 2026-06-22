// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! A thin OpenRouter HTTP client covering the calls the pipeline needs from OpenRouter:
//! structured chat, image generation, text-to-speech, music, and video. (Caption word timings
//! come from a local whisper-timestamped run, not OpenRouter — see `transcribe.rs`.)

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::config::Config;

const BASE: &str = "https://openrouter.ai/api/v1";

/// Audio returned by text-to-speech, tagged with its container format.
pub struct Speech {
    pub bytes: Vec<u8>,
    /// "mp3" or "pcm" (raw 24kHz mono signed-16-bit little-endian).
    pub format: String,
}

/// Generated music audio, tagged with its container format.
pub struct MusicTrack {
    pub bytes: Vec<u8>,
    /// Container format of `bytes` (always "wav" today — see `generate_music`).
    pub format: String,
}

/// The shared OpenRouter client. Holds one reusable `reqwest::Client` plus the resolved model
/// IDs and voice for this run, so every call site routes to the models the user configured
/// without re-reading `Config`. Construct once via `new` and pass it (by reference) through the
/// pipeline. `voice` is `pub` because `main.rs` may overwrite it after auto-picking from the
/// script's narrator gender.
pub struct OpenRouter {
    http: reqwest::Client,
    api_key: String,
    pub text_model: String,
    pub image_model: String,
    pub tts_model: String,
    pub music_model: String,
    pub video_model: String,
    pub voice: String,
}

impl OpenRouter {
    /// Build the client from resolved config. The HTTP client carries a generous 300s timeout
    /// because image/TTS/music generations are slow; video uses its own polling loop instead.
    /// `voice` defaults to "Kore" (a warm female voice) when none was configured.
    pub fn new(cfg: &Config) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()?;
        Ok(Self {
            http,
            api_key: cfg.api_key.clone(),
            text_model: cfg.text_model.clone(),
            image_model: cfg.image_model.clone(),
            tts_model: cfg.tts_model.clone(),
            music_model: cfg.music_model.clone(),
            video_model: cfg.video_model.clone(),
            voice: cfg.voice.clone().unwrap_or_else(|| "Kore".to_string()),
        })
    }

    /// Start a POST to `{BASE}{path}` with auth and the OpenRouter attribution headers
    /// (`HTTP-Referer`/`X-Title`) every call shares. Returns the builder so callers attach
    /// their own JSON body. The `Authorization: Bearer` header carries the API key.
    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.http
            .post(format!("{BASE}{path}"))
            .bearer_auth(&self.api_key)
            .header(
                "HTTP-Referer",
                "https://github.com/spunkytensor/reel-maestro",
            )
            .header("X-Title", "Reel Maestro")
    }

    /// Structured chat completion. The model is forced to return JSON matching `schema`,
    /// which we then parse into `T`.
    pub async fn chat_json<T: DeserializeOwned>(
        &self,
        system: &str,
        user: &str,
        schema_name: &str,
        schema: Value,
    ) -> Result<T> {
        let body = json!({
            "model": self.text_model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "response_format": {
                "type": "json_schema",
                "json_schema": { "name": schema_name, "strict": true, "schema": schema }
            }
        });
        let v = json_or_err(self.post("/chat/completions").json(&body).send().await?).await?;
        let content =
            message_text(&v).ok_or_else(|| anyhow!("no message content in chat response: {v}"))?;
        serde_json::from_str(&content).with_context(|| {
            format!("could not parse structured output as expected schema: {content}")
        })
    }

    /// Multimodal structured judgement: send `instruction` plus labeled images to the text model
    /// (which is multimodal) and parse its JSON reply against `schema` into `T`. Each image is
    /// preceded by its label, like `image_content`. Used by the consistency QA pass.
    pub async fn judge_json<T: DeserializeOwned>(
        &self,
        system: &str,
        instruction: &str,
        images: &[(String, String)],
        schema_name: &str,
        schema: Value,
    ) -> Result<T> {
        let mut content = vec![json!({ "type": "text", "text": instruction })];
        for (label, url) in images {
            content.push(json!({ "type": "text", "text": label }));
            content.push(json!({ "type": "image_url", "image_url": { "url": url } }));
        }
        let body = json!({
            "model": self.text_model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": content},
            ],
            "response_format": {
                "type": "json_schema",
                "json_schema": { "name": schema_name, "strict": true, "schema": schema }
            }
        });
        let v = json_or_err(self.post("/chat/completions").json(&body).send().await?).await?;
        let reply =
            message_text(&v).ok_or_else(|| anyhow!("no message content in judge response: {v}"))?;
        serde_json::from_str(&reply)
            .with_context(|| format!("could not parse judge output as expected schema: {reply}"))
    }

    /// Image generation through the chat-completions `modalities` path. Optional `references` are
    /// `(label, data_url)` pairs included as input images, each preceded by its label so the model
    /// knows which reference is which subject (person / location / prior frame). Returns raw bytes.
    pub async fn generate_image(
        &self,
        prompt: &str,
        references: &[(String, String)],
    ) -> Result<Vec<u8>> {
        let body = json!({
            "model": self.image_model,
            "messages": [{"role": "user", "content": image_content(prompt, references)}],
            "modalities": ["image", "text"],
        });
        let v = json_or_err(self.post("/chat/completions").json(&body).send().await?).await?;
        let msg = &v["choices"][0]["message"];

        // Generated images normally arrive as data URLs under message.images[].
        if let Some(url) = msg["images"][0]["image_url"]["url"]
            .as_str()
            .or_else(|| msg["images"][0]["url"].as_str())
        {
            return decode_data_url(url);
        }
        // Some providers embed a data URL inside the text content instead.
        if let Some(content) = msg["content"].as_str() {
            if let Some(start) = content.find("data:image") {
                let rest = &content[start..];
                let end = rest
                    .find(|c: char| c.is_whitespace() || c == ')' || c == '"')
                    .unwrap_or(rest.len());
                return decode_data_url(&rest[..end]);
            }
        }
        // No image — usually a soft refusal that returned plain text.
        let snippet: String = msg["content"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(160)
            .collect();
        bail!(
            "{} returned no image (model said: {snippet:?})",
            self.image_model
        )
    }

    /// Text-to-speech. Different providers accept different output formats (e.g. Gemini
    /// TTS is PCM-only, OpenAI supports mp3), so we pick a sensible primary by model and
    /// fall back to the other if the provider rejects it. Returns the audio bytes plus
    /// the negotiated format ("mp3" or "pcm").
    pub async fn text_to_speech(&self, text: &str) -> Result<Speech> {
        let primary = if self.tts_model.contains("tts") && self.tts_model.contains("gemini") {
            "pcm"
        } else {
            "mp3"
        };
        let alt = if primary == "mp3" { "pcm" } else { "mp3" };

        match self.tts_request(text, primary).await {
            Ok(bytes) => Ok(Speech { bytes, format: primary.into() }),
            Err(first) => match self.tts_request(text, alt).await {
                Ok(bytes) => Ok(Speech { bytes, format: alt.into() }),
                Err(second) => bail!(
                    "text-to-speech failed for {} (tried {primary} then {alt}):\n  {first}\n  {second}",
                    self.tts_model
                ),
            },
        }
    }

    /// One TTS attempt at a specific `response_format` ("mp3" or "pcm"). Surfaces a non-2xx
    /// response as an error so `text_to_speech` can fall back to the other format.
    async fn tts_request(&self, text: &str, response_format: &str) -> Result<Vec<u8>> {
        let body = json!({
            "model": self.tts_model,
            "input": text,
            "voice": self.voice,
            "response_format": response_format,
        });
        let resp = self.post("/audio/speech").json(&body).send().await?;
        let status = resp.status();
        if !status.is_success() {
            bail!("{status}: {}", resp.text().await.unwrap_or_default());
        }
        Ok(resp.bytes().await?.to_vec())
    }

    // NOTE: there is intentionally no OpenRouter speech-to-text call here. We verified against
    // the live API that OpenRouter normalizes every transcription model's response to plain
    // `text` + `usage` (no word/segment timestamps) and rejects multipart uploads, so it can't
    // drive caption timing. Word timings come from a local whisper-timestamped run in
    // `transcribe.rs`; the narration text we already have from the script.

    /// Generate instrumental music from a text prompt (e.g. Lyria 3). OpenRouter requires
    /// `stream: true` for audio output, so we read the SSE stream and gather the base64 audio
    /// payload wherever it appears in the chunks, then decode it. Returns bytes plus format.
    pub async fn generate_music(&self, prompt: &str) -> Result<MusicTrack> {
        let format = "wav".to_string();
        let body = json!({
            "model": self.music_model,
            "messages": [{"role": "user", "content": prompt}],
            "modalities": ["text", "audio"],
            "audio": { "format": "wav" },
            "stream": true,
        });
        let resp = self.post("/chat/completions").json(&body).send().await?;
        let status = resp.status();
        if !status.is_success() {
            bail!(
                "music generation failed ({status}): {}",
                resp.text().await.unwrap_or_default()
            );
        }

        use futures::StreamExt;
        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        let mut fragments: Vec<String> = Vec::new();
        let mut samples: Vec<String> = Vec::new();

        while let Some(chunk) = stream.next().await {
            buf.push_str(&String::from_utf8_lossy(&chunk?));
            while let Some(nl) = buf.find('\n') {
                let line: String = buf.drain(..=nl).collect();
                let line = line.trim();
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<Value>(data) else {
                    continue;
                };
                let before = fragments.len();
                collect_audio_fragments(&v, &mut fragments);
                // Keep a couple of audio-free chunks as a diagnostic sample.
                if before == fragments.len() && samples.len() < 2 {
                    samples.push(data.chars().take(300).collect());
                }
            }
        }

        if fragments.is_empty() {
            bail!(
                "music model {} streamed no audio. sample chunks: {}",
                self.music_model,
                samples.join(" || ")
            );
        }

        // Audio deltas concatenate as base64 then decode; fall back to per-fragment decode.
        let joined = fragments.concat();
        let bytes = match base64::engine::general_purpose::STANDARD.decode(joined.trim()) {
            Ok(b) => b,
            Err(_) => {
                let mut b = Vec::new();
                for f in &fragments {
                    if let Ok(part) = base64::engine::general_purpose::STANDARD.decode(f.trim()) {
                        b.extend(part);
                    }
                }
                if b.is_empty() {
                    bail!(
                        "could not decode streamed music audio from {} fragments",
                        fragments.len()
                    );
                }
                b
            }
        };
        Ok(MusicTrack { bytes, format })
    }

    /// Image-to-video (or text-to-video) via the async video jobs API: submit, poll until
    /// the job completes, then download the MP4. `first_frame` is a base64 data URL used as
    /// the first frame; pass `None` for pure text-to-video. Retries once as text-to-video
    /// if an image-conditioned submit fails, and retries a *terminally-failed* job once (Veo's
    /// safety filter is often non-deterministic, so a re-roll frequently succeeds).
    pub async fn generate_video(
        &self,
        prompt: &str,
        first_frame: Option<&str>,
        duration: u32,
        resolution: &str,
    ) -> Result<Vec<u8>> {
        const ATTEMPTS: usize = 2;
        let mut last_err = String::new();
        for attempt in 1..=ATTEMPTS {
            let polling_url = match self
                .submit_video(prompt, first_frame, duration, resolution)
                .await
            {
                Ok(url) => url,
                Err(e) if first_frame.is_some() => {
                    eprintln!("    image-to-video submit failed ({e}); retrying as text-to-video");
                    self.submit_video(prompt, None, duration, resolution)
                        .await?
                }
                Err(e) => return Err(e),
            };

            // Poll until done. Jobs take ~30s–several minutes; cap at ~10 minutes.
            let max_polls = 30;
            let mut failure: Option<String> = None;
            for _ in 0..max_polls {
                tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                match self.poll_video(&polling_url).await? {
                    VideoStatus::Pending => continue,
                    VideoStatus::Failed(msg) => {
                        failure = Some(msg);
                        break;
                    }
                    VideoStatus::Done(content_url) => {
                        return self.download_video(&content_url).await
                    }
                }
            }

            match failure {
                Some(msg) => {
                    last_err = msg;
                    // Only re-roll a generic, possibly non-deterministic content filter. A
                    // DETERMINISTIC block (e.g. the input frame contains a person/face Veo refuses
                    // to animate) will fail identically on re-submit, so a second attempt just wastes
                    // time and cost — fail fast instead.
                    if attempt < ATTEMPTS && !is_deterministic_block(&last_err) {
                        eprintln!(
                            "    video job failed ({last_err}); re-rolling once ({}/{ATTEMPTS})",
                            attempt + 1
                        );
                        continue;
                    }
                    bail!("video job failed: {last_err}");
                }
                None => bail!("video job timed out after {} minutes", max_polls * 20 / 60),
            }
        }
        bail!("video job failed: {last_err}")
    }

    /// Submit one video job and return the URL to poll for its result. Always requests a
    /// vertical 9:16 clip with no model-generated audio (we mix our own narration/music later).
    /// When `first_frame` is set it's attached as the clip's first frame for image-to-video.
    async fn submit_video(
        &self,
        prompt: &str,
        first_frame: Option<&str>,
        duration: u32,
        resolution: &str,
    ) -> Result<String> {
        let mut body = json!({
            "model": self.video_model,
            "prompt": prompt,
            "aspect_ratio": "9:16",
            "resolution": resolution,
            "duration": duration,
            "generate_audio": false,
        });
        if let Some(url) = first_frame {
            body["frame_images"] = json!([{
                "type": "image_url",
                "image_url": { "url": url },
                "frame_type": "first_frame",
            }]);
        }
        let v = json_or_err(self.post("/videos").json(&body).send().await?).await?;
        // Prefer an absolute polling_url; fall back to constructing one from the job id.
        if let Some(url) = v["polling_url"].as_str() {
            return Ok(url.to_string());
        }
        let id = v["id"]
            .as_str()
            .ok_or_else(|| anyhow!("video submit returned no id: {v}"))?;
        Ok(format!("{BASE}/videos/{id}"))
    }

    /// Poll a video job once and map its provider status to our `VideoStatus`. Terminal
    /// success yields a content URL (preferring `unsigned_urls[0]`, else one built from the
    /// job id); any not-yet-terminal status maps to `Pending` so the caller keeps polling.
    async fn poll_video(&self, polling_url: &str) -> Result<VideoStatus> {
        let resp = self
            .http
            .get(polling_url)
            .bearer_auth(&self.api_key)
            .send()
            .await?;
        let v = json_or_err(resp).await?;
        match v["status"].as_str().unwrap_or("") {
            "completed" | "succeeded" => {
                let url = v["unsigned_urls"][0]
                    .as_str()
                    .map(|s| s.to_string())
                    .or_else(|| {
                        v["id"]
                            .as_str()
                            .map(|id| format!("{BASE}/videos/{id}/content?index=0"))
                    })
                    .ok_or_else(|| anyhow!("completed video job had no content url: {v}"))?;
                Ok(VideoStatus::Done(url))
            }
            "failed" | "cancelled" | "expired" => Ok(VideoStatus::Failed(describe_video_error(&v))),
            _ => Ok(VideoStatus::Pending),
        }
    }

    /// Download the finished MP4 bytes from a completed job's content URL.
    async fn download_video(&self, content_url: &str) -> Result<Vec<u8>> {
        let resp = self
            .http
            .get(content_url)
            .bearer_auth(&self.api_key)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            bail!(
                "video download failed ({status}): {}",
                resp.text().await.unwrap_or_default()
            );
        }
        Ok(resp.bytes().await?.to_vec())
    }
}

/// The outcome of a single video-job poll: still running, finished (with the content URL to
/// download), or terminally failed (with the provider's error message).
enum VideoStatus {
    Pending,
    Done(String),
    Failed(String),
}

/// Build the most informative failure string we can from a terminal video-job response. Veo (and
/// other providers proxied by OpenRouter) report failures inconsistently — sometimes a plain
/// `error` string, sometimes a `{code, message}` object, and on safety blocks a separate set of
/// RAI media-filter fields. We pull whatever is present (so a content-filter reason isn't reduced
/// to "unknown error") and fall back to a compact raw dump when nothing structured is found.
/// Whether a video-job failure message is a DETERMINISTIC block (a safety/policy rejection of the
/// input, like a person/face the model refuses to animate) that will fail identically on re-submit,
/// versus a generic, possibly non-deterministic content filter worth one re-roll. Matched on the
/// human-readable reason from `describe_video_error`.
fn is_deterministic_block(msg: &str) -> bool {
    let m = msg.to_lowercase();
    m.contains("safety setting")
        || m.contains("person/face")
        || m.contains("support code")
        || m.contains("input image contains content")
        || (m.contains("blocked") && m.contains("input"))
}

fn describe_video_error(v: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();

    // `error` as a plain string, or a structured `{message, code}` object.
    match &v["error"] {
        Value::String(s) if !s.trim().is_empty() => parts.push(s.trim().to_string()),
        Value::Object(o) => {
            let msg = o
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            let code = o
                .get("code")
                .map(|c| {
                    c.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| c.to_string())
                })
                .unwrap_or_default();
            match (msg.is_empty(), code.is_empty()) {
                (false, false) => parts.push(format!("{msg} (code {code})")),
                (false, true) => parts.push(msg.to_string()),
                (true, false) => parts.push(format!("code {code}")),
                (true, true) => {}
            }
        }
        _ => {}
    }

    // Safety-filter specifics, when the provider includes them (Veo's RAI media filter). Accept a
    // couple of key spellings since the proxy may pass them through camelCase or snake_case.
    for key in ["raiMediaFilteredReasons", "rai_media_filtered_reasons"] {
        if let Some(arr) = v[key].as_array() {
            let reasons: Vec<String> = arr
                .iter()
                .filter_map(|r| r.as_str().map(str::to_string))
                .collect();
            if !reasons.is_empty() {
                parts.push(format!("safety filter: {}", reasons.join("; ")));
            }
        }
    }

    if parts.is_empty() {
        // Nothing structured — surface a compact raw payload so the reason isn't lost entirely.
        let raw: String = v.to_string().chars().take(400).collect();
        parts.push(format!("no error detail in response; raw: {raw}"));
    }
    parts.join(" — ")
}

/// Build a base64 `data:` URL from image bytes, sniffing the MIME type from the
/// magic bytes. Our own saved frames are always JPEG, but a user-supplied
/// `--character-ref` may be PNG/WebP/GIF, and mislabeling it makes the API reject
/// the request. Falls back to JPEG when the format isn't recognized.
pub fn data_url_from_image(bytes: &[u8]) -> String {
    let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if bytes.starts_with(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "image/jpeg"
    };
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// Collect base64 audio fragments from a streaming chunk: any string under a key named
/// "data" long enough to be an audio payload (not a short transcript field). Recurses, so it
/// works regardless of whether the audio sits under delta.audio, message.audio, content[], etc.
fn collect_audio_fragments(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                if k == "data" {
                    if let Some(s) = val.as_str() {
                        if s.len() > 64 {
                            out.push(s.to_string());
                        }
                    }
                }
                collect_audio_fragments(val, out);
            }
        }
        Value::Array(arr) => {
            for val in arr {
                collect_audio_fragments(val, out);
            }
        }
        _ => {}
    }
}

/// Read a response as JSON, turning a non-2xx status into an error that includes the body.
/// We read the body as text first so failures surface OpenRouter's message instead of an
/// opaque parse error. Shared by every non-streaming call.
async fn json_or_err(resp: reqwest::Response) -> Result<Value> {
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        bail!("OpenRouter request failed ({status}): {text}");
    }
    serde_json::from_str(&text).with_context(|| format!("invalid JSON from OpenRouter: {text}"))
}

/// Decode the base64 payload of a `data:<mime>;base64,<payload>` URL into raw bytes.
fn decode_data_url(url: &str) -> Result<Vec<u8>> {
    // Expected form: data:image/png;base64,<payload>
    let comma = url
        .find(',')
        .ok_or_else(|| anyhow!("malformed image data URL"))?;
    Ok(base64::engine::general_purpose::STANDARD.decode(&url[comma + 1..])?)
}

/// Build the chat message `content` for image generation. With no references it's a plain string
/// (unchanged behaviour); with references it's a multimodal array of the prompt followed by, for
/// EACH reference, a text label then its image — so the model binds each image to the right
/// subject (person / location / prior frame) instead of guessing the mapping from order alone.
/// Extract the assistant message text from a chat-completions response, tolerating providers that
/// return `content` as a plain string OR as an array of `{type:"text", text:...}` parts (common for
/// multimodal replies). Returns `None` when there is no text (e.g. a refusal with null content), so
/// the caller can surface a clear error instead of mis-reading the shape.
fn message_text(v: &Value) -> Option<String> {
    let content = &v["choices"][0]["message"]["content"];
    if let Some(s) = content.as_str() {
        return (!s.is_empty()).then(|| s.to_string());
    }
    if let Some(parts) = content.as_array() {
        let text: String = parts
            .iter()
            .filter_map(|p| p["text"].as_str().or_else(|| p.as_str()))
            .collect();
        return (!text.is_empty()).then_some(text);
    }
    None
}

fn image_content(prompt: &str, references: &[(String, String)]) -> Value {
    if references.is_empty() {
        return json!(prompt);
    }
    let mut parts = vec![json!({ "type": "text", "text": prompt })];
    for (label, url) in references {
        parts.push(json!({ "type": "text", "text": label }));
        parts.push(json!({ "type": "image_url", "image_url": { "url": url } }));
    }
    json!(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_content_no_refs_is_plain_string() {
        let c = image_content("a cat", &[]);
        assert_eq!(c, json!("a cat"));
    }

    #[test]
    fn deterministic_blocks_are_not_retried() {
        // Safety/input rejections (fail identically on re-submit) → no re-roll.
        assert!(is_deterministic_block(
            "The input image contains content that has been blocked by your current safety settings \
             for person/face generation. Support codes: 17301594"
        ));
        assert!(is_deterministic_block("blocked by your safety settings"));
        // A generic, possibly non-deterministic filter → still worth one re-roll.
        assert!(!is_deterministic_block(
            "Video generation completed with no output (content may have been filtered)"
        ));
        assert!(!is_deterministic_block("video job timed out"));
    }

    #[test]
    fn video_error_surfaces_string_object_and_filter_detail() {
        // Plain string error.
        assert_eq!(
            describe_video_error(&json!({"error": "content may have been filtered"})),
            "content may have been filtered"
        );
        // Structured {message, code} object.
        assert_eq!(
            describe_video_error(&json!({"error": {"message": "blocked", "code": 400}})),
            "blocked (code 400)"
        );
        // Safety-filter reasons get appended.
        let m = describe_video_error(&json!({
            "error": "filtered",
            "raiMediaFilteredReasons": ["Person/Face generation", "Celebrity"]
        }));
        assert!(m.contains("filtered"));
        assert!(m.contains("safety filter: Person/Face generation; Celebrity"));
        // No structured detail → compact raw dump instead of being lost.
        let m = describe_video_error(&json!({"status": "failed"}));
        assert!(m.contains("raw:"));
    }

    #[test]
    fn data_url_sniffs_mime_from_magic_bytes() {
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0];
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0, 0];
        let gif = *b"GIF89a";
        let webp = *b"RIFF\0\0\0\0WEBPVP8 ";
        let unknown = [0x00, 0x01, 0x02, 0x03];

        assert!(data_url_from_image(&png).starts_with("data:image/png;base64,"));
        assert!(data_url_from_image(&jpeg).starts_with("data:image/jpeg;base64,"));
        assert!(data_url_from_image(&gif).starts_with("data:image/gif;base64,"));
        assert!(data_url_from_image(&webp).starts_with("data:image/webp;base64,"));
        // Unrecognized bytes fall back to JPEG.
        assert!(data_url_from_image(&unknown).starts_with("data:image/jpeg;base64,"));
    }

    #[test]
    fn image_content_interleaves_label_then_image() {
        let c = image_content(
            "a cat",
            &[(
                "PERSON \"jake\"".to_string(),
                "data:image/jpeg;base64,AAAA".to_string(),
            )],
        );
        // prompt, then the reference's LABEL, then its image — so each image is bound to its subject.
        assert_eq!(c[0]["type"], "text");
        assert_eq!(c[0]["text"], "a cat");
        assert_eq!(c[1]["type"], "text");
        assert_eq!(c[1]["text"], "PERSON \"jake\"");
        assert_eq!(c[2]["type"], "image_url");
        assert_eq!(c[2]["image_url"]["url"], "data:image/jpeg;base64,AAAA");
    }

    #[test]
    fn collect_audio_fragments_finds_audio_anywhere() {
        let long = "Q".repeat(100); // stands in for a base64 audio payload
                                    // delta.audio.data
        let a = json!({"choices":[{"delta":{"audio":{"data": long}}}]});
        // message.audio.data
        let b = json!({"choices":[{"message":{"audio":{"data": long}}}]});
        // nested content array
        let c =
            json!({"choices":[{"delta":{"content":[{"type":"audio","audio":{"data": long}}]}}]});
        // a short transcript "data" must be ignored
        let d = json!({"choices":[{"delta":{"data":"hello"}}]});

        for chunk in [&a, &b, &c] {
            let mut out = Vec::new();
            collect_audio_fragments(chunk, &mut out);
            assert_eq!(out, vec![long.clone()]);
        }
        let mut out = Vec::new();
        collect_audio_fragments(&d, &mut out);
        assert!(out.is_empty(), "short non-audio data should be ignored");
    }
}
