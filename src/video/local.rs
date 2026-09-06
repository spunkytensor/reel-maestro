// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::{Config, VideoInputMode};

#[cfg(test)]
#[path = "local_tests.rs"]
mod tests;

#[derive(Deserialize)]
struct Discovery {
    data: Vec<ModelCapability>,
}

#[derive(Deserialize)]
struct ModelCapability {
    id: String,
    supported_durations: Vec<u32>,
    supported_sizes: Vec<String>,
    local: Value,
}

fn local_http() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()?)
}

fn local_token(cfg: &Config) -> &str {
    cfg.video_api_token.as_deref().unwrap_or_default()
}

// Authentication validation returns no credential-bearing value through the error/result path.
fn validate_local_auth(cfg: &Config) -> Result<()> {
    if local_token(cfg).is_empty() {
        bail!("local video authentication is missing; set REELMAESTRO_VIDEO_API_TOKEN (or H3_STUDIO_KEY)");
    }
    Ok(())
}

async fn local_json(cfg: &Config, path: &str) -> Result<Value> {
    let response = local_http()?
        .get(format!("{}{path}", cfg.video_base_url))
        .bearer_auth(local_token(cfg))
        .send()
        .await?;
    let status = response.status();
    let value: Value = response
        .json()
        .await
        .context("local H3 returned invalid JSON")?;
    if !status.is_success() {
        bail!("local H3 request failed ({status}): {value}");
    }
    Ok(value)
}

/// Fail before any fresh paid pipeline stage if the configured local service is unavailable or
/// does not advertise the exact contract Reel Maestro will use.
pub async fn preflight_local(cfg: &Config) -> Result<()> {
    validate_local_auth(cfg)?;
    let base = reqwest::Url::parse(&cfg.video_base_url)?;
    if !matches!(base.scheme(), "http" | "https")
        || base.host_str().is_none()
        || !base.username().is_empty()
        || base.password().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
        || base.path() != "/"
    {
        bail!("--video-base-url must be an HTTP(S) origin without credentials, path or query");
    }
    let health = local_json(cfg, "/health").await?;
    if health["http"] != "ready" {
        bail!("local H3 HTTP service is not ready: {health}");
    }
    if health["model_ready"] != true {
        println!("  local H3 model is cold; the worker will load it when a job starts");
    }
    let discovery: Discovery =
        serde_json::from_value(local_json(cfg, "/api/v1/videos/models").await?)?;
    let model = discovery
        .data
        .into_iter()
        .find(|m| m.id == "local/minimax-h3")
        .ok_or_else(|| anyhow!("local H3 discovery did not advertise local/minimax-h3"))?;
    if !model.supported_sizes.contains(&cfg.video_resolution)
        || !model.supported_durations.contains(&5)
        || !model.supported_durations.contains(&10)
        || model.local["metered_charge"] != false
    {
        bail!(
            "local H3 discovery is incompatible with requested size/durations or metering contract"
        );
    }
    let min = model.local["steps"]["min"].as_u64().unwrap_or(2);
    let max = model.local["steps"]["max"].as_u64().unwrap_or(100);
    if !(min..=max).contains(&(cfg.video_steps as u64)) {
        bail!(
            "--video-steps {} is outside the discovered range {min}..={max}",
            cfg.video_steps
        );
    }
    let mode = match cfg.video_input_mode {
        VideoInputMode::FirstFrame => "frames",
        VideoInputMode::Text => "text",
    };
    if !model.local["input_modes"]
        .as_array()
        .is_some_and(|modes| modes.iter().any(|value| value == mode))
    {
        bail!("local H3 model does not advertise requested input mode {mode}");
    }
    Ok(())
}

#[derive(Debug)]
pub(super) enum LocalOutcome {
    Complete(PathBuf),
    // Only fixed diagnostic text crosses the logging boundary, never authenticated responses.
    Pending(&'static str),
}

#[derive(Serialize, Deserialize)]
struct LocalManifest {
    version: u8,
    provider: String,
    provider_origin: String,
    model: String,
    input_fingerprint: String,
    idempotency_key: String,
    request: Value,
    #[serde(default)]
    job_id: Option<String>,
    #[serde(default)]
    polling_url: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    metadata: Value,
}

fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let temp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(value)?;
    let mut file = std::fs::File::create(&temp)?;
    use std::io::Write;
    file.write_all(&bytes)?;
    file.sync_all()?;
    std::fs::rename(temp, path)?;
    std::fs::File::open(path.parent().unwrap_or_else(|| Path::new(".")))?.sync_all()?;
    Ok(())
}

fn same_origin(cfg: &Config, url: &str) -> Result<String> {
    let base = reqwest::Url::parse(&cfg.video_base_url)?;
    let parsed = base.join(url)?;
    if parsed.scheme() != base.scheme()
        || parsed.host_str() != base.host_str()
        || parsed.port_or_known_default() != base.port_or_known_default()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        bail!("local H3 returned a cross-origin URL; refusing to forward credentials");
    }
    Ok(parsed.to_string())
}

pub(super) async fn generate_local_clip(
    cfg: &Config,
    scene: usize,
    prompt: &str,
    image: &Path,
    duration: u32,
    size: &str,
    dir: &Path,
) -> Result<LocalOutcome> {
    validate_local_auth(cfg)?;
    let manifest_path = dir.join(format!("scene-{scene:02}.video-job.json"));
    let image_bytes = if cfg.video_input_mode == VideoInputMode::FirstFrame {
        Some(std::fs::read(image)?)
    } else {
        None
    };
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    cfg.video_input_mode.hash(&mut hasher);
    "local/minimax-h3".hash(&mut hasher);
    prompt.hash(&mut hasher);
    duration.hash(&mut hasher);
    size.hash(&mut hasher);
    cfg.video_seed.hash(&mut hasher);
    cfg.video_steps.hash(&mut hasher);
    image_bytes.hash(&mut hasher);
    let fingerprint = format!("{:016x}", hasher.finish());
    let mut manifest: LocalManifest = if manifest_path.exists() {
        let stored: LocalManifest = serde_json::from_slice(&std::fs::read(&manifest_path)?)?;
        if stored.input_fingerprint != fingerprint {
            bail!("local video inputs changed for scene {scene}; delete {} to intentionally create a new job", manifest_path.display());
        }
        if stored.provider_origin != cfg.video_base_url {
            bail!("local video endpoint changed from {} to {}; refusing to send credentials or resume this manifest", stored.provider_origin, cfg.video_base_url);
        }
        stored
    } else {
        let frame_url = if cfg.video_input_mode == VideoInputMode::FirstFrame {
            let bytes = image_bytes.clone().unwrap();
            let mime = if bytes.starts_with(b"\x89PNG") {
                "image/png"
            } else {
                "image/jpeg"
            };
            let response = local_http()?
                .post(format!("{}/api/v1/videos/images", cfg.video_base_url))
                .bearer_auth(local_token(cfg))
                .header("content-type", mime)
                .body(bytes)
                .send()
                .await?;
            let status = response.status();
            let value: Value = response.json().await?;
            if !status.is_success() {
                bail!("local H3 image upload failed ({status}): {value}");
            }
            Some(same_origin(
                cfg,
                value["url"]
                    .as_str()
                    .ok_or_else(|| anyhow!("upload returned no URL"))?,
            )?)
        } else {
            None
        };
        let mut request = json!({"model":"local/minimax-h3","prompt":prompt,"duration":duration,"size":size,"seed":cfg.video_seed,"generate_audio":false,"local":{"export":"original"},"provider":{"options":{"local":{"parameters":{"num_inference_steps":cfg.video_steps}}}}});
        if let Some(url) = frame_url {
            request["frame_images"] =
                json!([{"type":"image_url","image_url":{"url":url},"frame_type":"first_frame"}]);
        }
        let value = LocalManifest {
            version: 1,
            provider: "local".into(),
            provider_origin: cfg.video_base_url.clone(),
            model: "local/minimax-h3".into(),
            input_fingerprint: fingerprint.clone(),
            idempotency_key: {
                let canonical = std::fs::canonicalize(dir)?;
                let mut nonce = [0_u8; 16];
                std::io::Read::read_exact(&mut std::fs::File::open("/dev/urandom")?, &mut nonce)?;
                format!(
                    "reelmaestro-{scene}-{:016x}-{}",
                    {
                        let mut h = std::collections::hash_map::DefaultHasher::new();
                        canonical.hash(&mut h);
                        h.finish()
                    },
                    nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
                )
            },
            request,
            job_id: None,
            polling_url: None,
            status: Some("prepared".into()),
            metadata: Value::Null,
        };
        atomic_json(&manifest_path, &value)?;
        value
    };
    if manifest.job_id.is_none() {
        // Repeating this POST after an uncertain transport result reconciles the same durable key.
        let response = local_http()?
            .post(format!("{}/api/v1/videos", cfg.video_base_url))
            .bearer_auth(local_token(cfg))
            .header("Idempotency-Key", &manifest.idempotency_key)
            .json(&manifest.request)
            .send()
            .await?;
        let status = response.status();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());
        let value: Value = response.json().await?;
        if status.as_u16() == 429 {
            manifest.status = Some("submission_throttled".into());
            manifest.metadata = json!({"retry_after_seconds": retry_after});
            atomic_json(&manifest_path, &manifest)?;
            return Ok(LocalOutcome::Pending(
                "submission throttled; consult the manifest's retry_after_seconds and resume using the same key",
            ));
        }
        if !status.is_success() {
            bail!("local H3 submission failed ({status}): {value}");
        }
        manifest.job_id = Some(
            value["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| anyhow!("accepted job returned no job ID"))?
                .to_owned(),
        );
        manifest.polling_url = Some(same_origin(
            cfg,
            value["polling_url"]
                .as_str()
                .ok_or_else(|| anyhow!("accepted job returned no polling URL"))?,
        )?);
        manifest.status = Some("accepted".into());
        atomic_json(&manifest_path, &manifest)?;
    }
    let poll = manifest
        .polling_url
        .clone()
        .ok_or_else(|| anyhow!("manifest has no polling URL"))?;
    let poll = same_origin(cfg, &poll)?;
    let deadline = Instant::now() + Duration::from_secs(cfg.video_wait_timeout.saturating_mul(60));
    loop {
        let response = match local_http()?
            .get(&poll)
            .bearer_auth(local_token(cfg))
            .send()
            .await
        {
            Ok(response) => response,
            Err(_) => {
                if Instant::now() >= deadline {
                    return Ok(LocalOutcome::Pending(
                        "polling connection unavailable; accepted job retained for resume",
                    ));
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };
        let status = response.status();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(5)
            .max(1);
        if status.as_u16() == 429 || status.is_server_error() {
            if Duration::from_secs(retry_after)
                >= deadline.saturating_duration_since(Instant::now())
            {
                manifest.metadata = json!({"retry_after_seconds": retry_after});
                atomic_json(&manifest_path, &manifest)?;
                return Ok(LocalOutcome::Pending(
                    "polling delayed beyond the wait budget; consult retry_after_seconds in the manifest and resume",
                ));
            }
            tokio::time::sleep(Duration::from_secs(retry_after)).await;
            continue;
        }
        if !status.is_success() {
            bail!("local H3 poll failed ({status}); accepted job retained for resume");
        }
        let value: Value = match response.json().await {
            Ok(value) => value,
            Err(_) => {
                if Instant::now() >= deadline {
                    return Ok(LocalOutcome::Pending(
                        "invalid polling response; accepted job retained for resume",
                    ));
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };
        if manifest.status.as_deref() != value["status"].as_str()
            || manifest.metadata["phase"] != value["local"]["phase"]
            || manifest.metadata["progress"]["completed"] != value["local"]["progress"]["completed"]
        {
            let phase = value["local"]["phase"].as_str().unwrap_or("waiting");
            let state = value["status"].as_str().unwrap_or("unknown");
            let progress = match (
                value["local"]["progress"]["completed"].as_u64(),
                value["local"]["progress"]["total"].as_u64(),
            ) {
                (Some(done), Some(total)) => format!(", denoising {done}/{total}"),
                _ => String::new(),
            };
            println!("  scene {scene}: H3 {state}, {phase}{progress}");
        }
        manifest.status = value["status"].as_str().map(str::to_owned);
        manifest.metadata = value["local"].clone();
        atomic_json(&manifest_path, &manifest)?;
        match value["status"].as_str().unwrap_or("pending") {
            "completed" => {
                let content = same_origin(
                    cfg,
                    value["unsigned_urls"][0]
                        .as_str()
                        .ok_or_else(|| anyhow!("completed job returned no content URL"))?,
                )?;
                let temp = dir.join(format!(".scene-{scene:02}.mp4.tmp"));
                let response = local_http()?
                    .get(content)
                    .bearer_auth(local_token(cfg))
                    .send()
                    .await?;
                if !response.status().is_success() {
                    bail!("local H3 download failed ({})", response.status());
                }
                std::fs::write(&temp, response.bytes().await?)?;
                validate_mp4(&temp, size, duration)?;
                let final_path = dir.join(format!("scene-{scene:02}.mp4"));
                std::fs::rename(&temp, &final_path)?;
                manifest.status = Some("completed".into());
                atomic_json(&manifest_path, &manifest)?;
                return Ok(LocalOutcome::Complete(final_path));
            }
            "failed" | "cancelled" | "expired" => {
                manifest.status = Some(value["status"].as_str().unwrap().into());
                atomic_json(&manifest_path, &manifest)?;
                bail!(
                    "accepted local job {} failed: {}",
                    manifest.job_id.as_deref().unwrap_or("unknown"),
                    value["error"]
                );
            }
            _ if Instant::now() >= deadline => {
                return Ok(LocalOutcome::Pending(
                    "accepted job is still pending; its ID is retained in the manifest (resume with the same run folder)",
                ))
            }
            _ => tokio::time::sleep(Duration::from_secs(5)).await,
        }
    }
}

fn validate_mp4(path: &Path, size: &str, expected_duration: u32) -> Result<()> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name,codec_type,width,height,nb_frames,r_frame_rate:format=duration",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .context("failed to launch ffprobe")?;
    let probe: Value =
        serde_json::from_slice(&output.stdout).context("ffprobe returned invalid JSON")?;
    let streams = probe["streams"]
        .as_array()
        .ok_or_else(|| anyhow!("ffprobe returned no streams"))?;
    let video = streams
        .iter()
        .find(|s| s["codec_type"] == "video")
        .ok_or_else(|| anyhow!("downloaded local result has no video stream"))?;
    let (width, height) = size
        .split_once('x')
        .ok_or_else(|| anyhow!("invalid configured video size {size}"))?;
    let duration = probe["format"]["duration"]
        .as_str()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0);
    let frames = if expected_duration == 5 { 124 } else { 243 };
    if !output.status.success()
        || video["codec_name"] != "h264"
        || video["width"].as_u64() != width.parse().ok()
        || video["height"].as_u64() != height.parse().ok()
        || video["r_frame_rate"] != "24/1"
        || video["nb_frames"]
            .as_str()
            .and_then(|v| v.parse::<u32>().ok())
            != Some(frames)
        || !duration.is_finite()
        || (duration - f64::from(frames) / 24.0).abs() > 0.2
        || streams.iter().any(|s| s["codec_type"] == "audio")
    {
        bail!(
            "downloaded local result violates expected H.264/{size}/{expected_duration}s/no-audio contract: {probe}"
        );
    }
    let decoded = Command::new("ffmpeg")
        .args(["-v", "error", "-xerror", "-i"])
        .arg(path)
        .args(["-f", "null", "-"])
        .output()
        .context("failed to launch ffmpeg for full decode validation")?;
    if !decoded.status.success() {
        bail!(
            "downloaded local result failed full decode: {}",
            String::from_utf8_lossy(&decoded.stderr)
        );
    }
    Ok(())
}
