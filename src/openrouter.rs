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
    pub format: String,
}

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
        let content = v["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow!("no message content in chat response: {v}"))?;
        serde_json::from_str(content).with_context(|| {
            format!("could not parse structured output as expected schema: {content}")
        })
    }

    /// Image generation through the chat-completions `modalities` path. Optional `references`
    /// are base64 data URLs included as input images so the model can keep their subjects
    /// consistent (e.g. a recurring character). Returns raw image bytes.
    pub async fn generate_image(&self, prompt: &str, references: &[String]) -> Result<Vec<u8>> {
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
    /// if an image-conditioned submit fails.
    pub async fn generate_video(
        &self,
        prompt: &str,
        first_frame: Option<&str>,
        duration: u32,
        resolution: &str,
    ) -> Result<Vec<u8>> {
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
        for _ in 0..max_polls {
            tokio::time::sleep(std::time::Duration::from_secs(20)).await;
            match self.poll_video(&polling_url).await? {
                VideoStatus::Pending => continue,
                VideoStatus::Failed(msg) => bail!("video job failed: {msg}"),
                VideoStatus::Done(content_url) => return self.download_video(&content_url).await,
            }
        }
        bail!("video job timed out after {} minutes", max_polls * 20 / 60)
    }

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
            "failed" | "cancelled" | "expired" => Ok(VideoStatus::Failed(
                v["error"].as_str().unwrap_or("unknown error").to_string(),
            )),
            _ => Ok(VideoStatus::Pending),
        }
    }

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

enum VideoStatus {
    Pending,
    Done(String),
    Failed(String),
}

/// Build a base64 `data:` URL from JPEG bytes (used as a video first frame).
pub fn data_url_from_jpeg(bytes: &[u8]) -> String {
    format!(
        "data:image/jpeg;base64,{}",
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

async fn json_or_err(resp: reqwest::Response) -> Result<Value> {
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        bail!("OpenRouter request failed ({status}): {text}");
    }
    serde_json::from_str(&text).with_context(|| format!("invalid JSON from OpenRouter: {text}"))
}

fn decode_data_url(url: &str) -> Result<Vec<u8>> {
    // Expected form: data:image/png;base64,<payload>
    let comma = url
        .find(',')
        .ok_or_else(|| anyhow!("malformed image data URL"))?;
    Ok(base64::engine::general_purpose::STANDARD.decode(&url[comma + 1..])?)
}

/// Build the chat message `content` for image generation. With no references it's a plain
/// string (unchanged behaviour); with references it's a multimodal array of a text part plus
/// one `image_url` part per reference (data URL), which the model conditions on.
fn image_content(prompt: &str, references: &[String]) -> Value {
    if references.is_empty() {
        return json!(prompt);
    }
    let mut parts = vec![json!({ "type": "text", "text": prompt })];
    for url in references {
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
    fn image_content_with_refs_is_multimodal_array() {
        let c = image_content("a cat", &["data:image/jpeg;base64,AAAA".to_string()]);
        assert_eq!(c[0]["type"], "text");
        assert_eq!(c[0]["text"], "a cat");
        assert_eq!(c[1]["type"], "image_url");
        assert_eq!(c[1]["image_url"]["url"], "data:image/jpeg;base64,AAAA");
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
