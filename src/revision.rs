// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Immutable, dependency-aware revision planning and execution.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use clap::Parser;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::config::{self, Config, VideoInputMode, VideoProvider};
use crate::model::{Scene, Script};
use crate::{Cli, MusicAction};

const MANIFEST: &str = "run-manifest.json";
const PLAN_COPY: &str = "approved-plan.json";
const EXECUTION_STATE: &str = ".revision-execution.json";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum LegacyReuse {
    Reject,
    Trust,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct RevisionRequest {
    version: u8,
    source: PathBuf,
    revision_id: String,
    #[serde(default)]
    script: Option<Script>,
    #[serde(default)]
    arguments: Vec<String>,
    legacy_reuse: LegacyReuse,
    #[serde(default)]
    job_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ActionKind {
    Reuse,
    Generate,
    Derive,
    Exclude,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct ArtifactAction {
    artifact: String,
    action: ActionKind,
    reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    destination: Option<String>,
    dependencies: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct PlanCost {
    total_usd: f64,
    script_usd: f64,
    narration_usd: f64,
    images_usd: f64,
    video_usd: f64,
    music_usd: f64,
    uncertain: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct RevisionPlan {
    version: u8,
    source: PathBuf,
    destination: PathBuf,
    request: RevisionRequest,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_revision: Option<String>,
    source_snapshot: BTreeMap<String, String>,
    resolved_config: Value,
    actions: Vec<ArtifactAction>,
    cost: PlanCost,
    local_work: bool,
    warnings: Vec<String>,
    approval_hash: String,
}

#[derive(Serialize, Deserialize)]
struct RunManifest {
    version: u8,
    state: String,
    run_id: String,
    revision_id: String,
    parent_revision: Option<String>,
    approval_hash: String,
    source: PathBuf,
    config: Value,
    artifacts: BTreeMap<String, String>,
    actions: Vec<ArtifactAction>,
    warnings: Vec<String>,
    outputs: Vec<OutputArtifact>,
}

#[derive(Serialize, Deserialize)]
struct OutputArtifact {
    kind: String,
    path: String,
    sha256: String,
    bytes: u64,
}

#[derive(Serialize, Deserialize)]
struct ExecutionState {
    version: u8,
    approval_hash: String,
    state: String,
}

pub fn print_plan(path: &Path) -> Result<()> {
    let request: RevisionRequest = serde_json::from_slice(
        &fs::read(path).with_context(|| format!("could not read {}", path.display()))?,
    )
    .context("invalid revision request JSON")?;
    let plan = make_plan(request)?;
    println!("{}", serde_json::to_string(&plan)?);
    Ok(())
}

pub fn inspect_run(path: &Path) -> Result<()> {
    let source = fs::canonicalize(path)
        .with_context(|| format!("run source {} does not exist", path.display()))?;
    let mut script: Script = serde_json::from_slice(&fs::read(source.join("script.json"))?)
        .context("invalid source script.json")?;
    script.normalize_entities();
    script.ensure_scene_ids();
    let source_snapshot = snapshot(&source)?;
    let source_fingerprint = hash_json(&source_snapshot)?;
    let manifest = read_manifest(&source)?;
    let manifest_state = manifest
        .as_ref()
        .map(|manifest| manifest.state.as_str())
        .unwrap_or("legacy");
    println!(
        "{}",
        serde_json::to_string(&json!({
            "version": 1,
            "source": source,
            "source_fingerprint": source_fingerprint,
            "source_snapshot": source_snapshot,
            "manifest_state": manifest_state,
            "script": script,
            "outputs": discover_outputs(&source)?,
        }))?
    );
    Ok(())
}

fn make_plan(mut request: RevisionRequest) -> Result<RevisionPlan> {
    if request.version != 1 {
        bail!("unsupported revision request version {}", request.version);
    }
    validate_revision_id(&request.revision_id)?;
    reject_symlink_dir(&request.source, "source")?;
    let source = fs::canonicalize(&request.source).with_context(|| {
        format!(
            "revision source {} does not exist",
            request.source.display()
        )
    })?;
    request.source = source.clone();
    let manifest = read_manifest(&source)?;
    if manifest.is_none() && !matches!(request.legacy_reuse, LegacyReuse::Trust) {
        bail!(
            "legacy source has unknown provenance; set legacy_reuse to trust to acknowledge reuse"
        );
    }

    let mut prior: Script = serde_json::from_slice(&fs::read(source.join("script.json"))?)
        .context("invalid source script.json")?;
    prior.normalize_entities();
    prior.ensure_scene_ids();
    let mut desired = request.script.clone().unwrap_or_else(|| prior.clone());
    desired.normalize_entities();
    desired.ensure_scene_ids();
    validate_script(&desired)?;
    request.script = Some(desired.clone());

    let cli = parse_revision_args(&source, &request.arguments, true, manifest.as_ref())?;
    validate_scene_flags(&cli, &desired)?;
    let mut cfg = Config::load(&cli)?;
    cfg.format = if desired.format == "youtube" {
        config::Format::Youtube
    } else {
        config::Format::Reel
    };
    cfg.apply_video_format(cfg.format);
    if cfg.video_provider == VideoProvider::Openrouter
        && !cfg.video_model_explicit
        && cfg.format == config::Format::Youtube
    {
        cfg.video_model = "alibaba/wan-2.6".into();
    }
    request.arguments = effective_revision_arguments(&cli, &cfg);
    let resolved_config = config_value(&cli, &cfg);
    let mut snapshot = snapshot(&source)?;
    for (label, path) in [
        ("watermark", cli.watermark.as_deref()),
        ("music", cli.music.as_deref()),
        ("character_ref", cli.character_ref.as_deref()),
    ] {
        if let Some(path) = path {
            snapshot.insert(format!("external:{label}"), hash_file(path)?);
        }
    }
    let run_root = if source
        .parent()
        .and_then(Path::file_name)
        .and_then(|s| s.to_str())
        == Some("revisions")
    {
        source
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .to_path_buf()
    } else {
        source.clone()
    };
    let destination = run_root.join("revisions").join(&request.revision_id);
    let parent_revision = manifest.as_ref().map(|m| m.revision_id.clone());
    let (actions, estimate, warnings) =
        build_actions(&source, &prior, &desired, &cli, &cfg, manifest.as_ref())?;
    let mut plan = RevisionPlan {
        version: 1,
        source,
        destination,
        request,
        parent_revision,
        source_snapshot: snapshot,
        resolved_config,
        actions,
        cost: estimate,
        local_work: true,
        warnings,
        approval_hash: String::new(),
    };
    plan.approval_hash = plan_hash(&plan)?;
    Ok(plan)
}

fn parse_revision_args(
    source: &Path,
    args: &[String],
    planning: bool,
    manifest: Option<&RunManifest>,
) -> Result<Cli> {
    const FORBIDDEN: &[&str] = &[
        "--topic",
        "--brief",
        "--script",
        "--url",
        "--from",
        "--out",
        "--studio-schema",
        "--studio-estimate",
        "--studio-inspect-run",
        "--revision-plan",
        "--revision-execute",
        "--revision-recover",
        "--approval-hash",
        "--events-json",
        "--dry-run",
    ];
    for arg in args {
        if FORBIDDEN
            .iter()
            .any(|flag| arg == flag || arg.starts_with(&format!("{flag}=")))
        {
            bail!("revision arguments may not contain {arg}");
        }
    }
    let mut argv = vec![
        "reelmaestro".to_string(),
        "--no-dotenv".to_string(),
        "--from".to_string(),
    ];
    argv.push(source.display().to_string());
    argv.extend(args.iter().cloned());
    let mut cli = Cli::try_parse_from(argv).context("invalid revision arguments")?;
    cli.studio_estimate = planning;
    if let Some(manifest) = manifest {
        inherit_parent_config(&mut cli, args, manifest)?;
    }
    Ok(cli)
}

fn inherit_parent_config(cli: &mut Cli, args: &[String], manifest: &RunManifest) -> Result<()> {
    let old = &manifest.config;
    let has = |flag: &str| {
        args.iter()
            .any(|arg| arg == flag || arg.starts_with(&format!("{flag}=")))
    };
    let text = |key: &str| old[key].as_str().map(str::to_owned);
    if !has("--voice") {
        cli.voice = text("voice");
    }
    if !has("--speed") {
        if let Some(v) = old["speed"].as_f64() {
            cli.speed = v;
        }
    }
    if !has("--minutes") {
        cli.minutes = old["minutes"].as_f64();
    }
    for (flag, target, key) in [
        ("--text-model", &mut cli.text_model, "text_model"),
        ("--image-model", &mut cli.image_model, "image_model"),
        ("--judge-model", &mut cli.judge_model, "judge_model"),
        ("--tts-model", &mut cli.tts_model, "tts_model"),
        ("--music-model", &mut cli.music_model, "music_model"),
        ("--video-model", &mut cli.video_model, "video_model"),
        ("--caption-font", &mut cli.caption_font, "caption_font"),
        ("--whisper-cmd", &mut cli.whisper_cmd, "whisper_cmd"),
        ("--whisper-model", &mut cli.whisper_model, "whisper_model"),
    ] {
        if !has(flag) {
            *target = text(key);
        }
    }
    if !has("--validate-scene") {
        cli.validate_scene = old["validate_scene"].as_u64().map(|v| v as usize);
    }
    if !has("--scene-seconds") {
        cli.scene_seconds = old["scene_seconds"].as_f64();
    }
    if !has("--dissolve-seconds") {
        if let Some(v) = old["dissolve_seconds"].as_f64() {
            cli.dissolve_seconds = v;
        }
    }
    if !has("--music-volume") {
        if let Some(v) = old["music_volume"].as_f64() {
            cli.music_volume = v;
        }
    }
    if !has("--poster-scene") {
        cli.poster_scene = old["poster_scene"].as_u64().map(|v| v as usize);
    }
    if old["video_provider"] == "local" {
        if !has("--video-seed") {
            cli.video_seed = old["video_seed"].as_u64();
        }
        if !has("--video-steps") {
            cli.video_steps = old["video_steps"].as_u64().map(|v| v as u32);
        }
        if !has("--video-wait-timeout") {
            cli.video_wait_timeout = old["video_wait_timeout"].as_u64();
        }
        if !has("--video-base-url") {
            cli.video_base_url = text("video_base_url");
        }
    }
    if !has("--video-provider") {
        cli.video_provider = match old["video_provider"].as_str() {
            Some("local") => Some(VideoProvider::Local),
            Some("openrouter") => Some(VideoProvider::Openrouter),
            _ => None,
        };
    }
    if old["video_provider"] == "local" && !has("--video-input-mode") {
        cli.video_input_mode = match old["video_input_mode"].as_str() {
            Some("text") => Some(VideoInputMode::Text),
            Some("firstframe" | "first-frame") => Some(VideoInputMode::FirstFrame),
            _ => None,
        };
    }
    if !has("--video-resolution") && !has("--video-size") {
        if cli.video_provider == Some(VideoProvider::Local) {
            cli.video_size = text("video_resolution");
        } else {
            cli.video_resolution = text("video_resolution");
        }
    }
    if !has("--caption-style") {
        cli.caption_style = old["caption_style"].as_str().and_then(|v| {
            <crate::captions::CaptionPreset as clap::ValueEnum>::from_str(v, true).ok()
        });
    }
    if !has("--mix") {
        cli.mix = match old["mix"].as_str() {
            Some("low") => crate::MixMode::Low,
            _ => crate::MixMode::Duck,
        };
    }
    if !has("--export-preset") {
        cli.export_preset = match old["export_preset"].as_str() {
            Some("web") => crate::ExportPreset::Web,
            Some("pro") => crate::ExportPreset::Pro,
            _ => crate::ExportPreset::Social,
        };
    }
    if !has("--character-ref") {
        cli.character_ref = text("character_ref").map(PathBuf::from);
    }
    if !has("--watermark") {
        cli.watermark = text("watermark").map(PathBuf::from);
    }
    inherit_toggle(
        args,
        "--captions",
        "--no-captions",
        !old["no_captions"].as_bool().unwrap_or(false),
        &mut cli.captions,
        &mut cli.no_captions,
    );
    inherit_toggle(
        args,
        "--narration",
        "--no-narration",
        !old["no_narration"].as_bool().unwrap_or(false),
        &mut cli.narration,
        &mut cli.no_narration,
    );
    inherit_toggle(
        args,
        "--dissolve",
        "--no-dissolve",
        old["dissolve"].as_bool().unwrap_or(true),
        &mut cli.dissolve,
        &mut cli.no_dissolve,
    );
    inherit_toggle(
        args,
        "--grade",
        "--no-grade",
        old["grade"].as_bool().unwrap_or(true),
        &mut cli.grade,
        &mut cli.no_grade,
    );
    inherit_toggle(
        args,
        "--loudnorm",
        "--no-loudnorm",
        old["loudnorm"].as_bool().unwrap_or(true),
        &mut cli.loudnorm,
        &mut cli.no_loudnorm,
    );
    inherit_toggle(
        args,
        "--embed-poster",
        "--no-embed-poster",
        old["embed_poster"].as_bool().unwrap_or(true),
        &mut cli.embed_poster,
        &mut cli.no_embed_poster,
    );
    inherit_toggle(
        args,
        "--consistency",
        "--no-consistency",
        old["consistency"].as_bool().unwrap_or(true),
        &mut cli.consistency,
        &mut cli.no_consistency,
    );
    if !has("--video-scene-id") && !has("--video") && !has("--video-scenes") {
        cli.video_scene_ids = manifest
            .actions
            .iter()
            .filter(|action| {
                action.artifact.starts_with("scene:")
                    && action.artifact.ends_with(":video")
                    && action.action != ActionKind::Exclude
            })
            .filter_map(|action| {
                action
                    .artifact
                    .strip_prefix("scene:")?
                    .strip_suffix(":video")
                    .map(str::to_owned)
            })
            .collect();
    }
    Ok(())
}

fn inherit_toggle(
    args: &[String],
    positive: &str,
    negative: &str,
    enabled: bool,
    positive_target: &mut bool,
    negative_target: &mut bool,
) {
    if args.iter().any(|arg| arg == positive || arg == negative) {
        return;
    }
    *positive_target = enabled;
    *negative_target = !enabled;
}

fn effective_revision_arguments(cli: &Cli, cfg: &Config) -> Vec<String> {
    let toggles = [
        (!cfg.no_captions, "--captions", "--no-captions"),
        (!cfg.no_narration, "--narration", "--no-narration"),
        (
            cli.dissolve || !cli.no_dissolve,
            "--dissolve",
            "--no-dissolve",
        ),
        (cli.grade || !cli.no_grade, "--grade", "--no-grade"),
        (
            cli.loudnorm || !cli.no_loudnorm,
            "--loudnorm",
            "--no-loudnorm",
        ),
        (
            cli.embed_poster || !cli.no_embed_poster,
            "--embed-poster",
            "--no-embed-poster",
        ),
        (
            cli.consistency || !cli.no_consistency,
            "--consistency",
            "--no-consistency",
        ),
    ];
    let mut args: Vec<String> = toggles
        .into_iter()
        .map(|(enabled, positive, negative)| if enabled { positive } else { negative }.into())
        .collect();
    let mut value = |flag: &str, value: String| {
        args.push(flag.into());
        args.push(value);
    };
    if let Some(voice) = &cfg.voice {
        value("--voice", voice.clone());
    }
    value("--speed", cli.speed.to_string());
    value("--format", format!("{:?}", cfg.format).to_lowercase());
    if cfg.format == config::Format::Youtube {
        value("--minutes", cfg.minutes.to_string());
    }
    value("--text-model", cfg.text_model.clone());
    value("--image-model", cfg.image_model.clone());
    value("--judge-model", cfg.judge_model.clone());
    value("--tts-model", cfg.tts_model.clone());
    value("--music-model", cfg.music_model.clone());
    value("--video-model", cfg.video_model.clone());
    value(
        "--video-provider",
        format!("{:?}", cfg.video_provider).to_lowercase(),
    );
    if cfg.video_provider == VideoProvider::Local {
        value("--video-size", cfg.video_resolution.clone());
        value("--video-base-url", cfg.video_base_url.clone());
        value(
            "--video-input-mode",
            match cfg.video_input_mode {
                VideoInputMode::FirstFrame => "first-frame",
                VideoInputMode::Text => "text",
            }
            .into(),
        );
        value("--video-seed", cfg.video_seed.to_string());
        value("--video-steps", cfg.video_steps.to_string());
        value("--video-wait-timeout", cfg.video_wait_timeout.to_string());
    } else {
        value("--video-resolution", cfg.video_resolution.clone());
    }
    value(
        "--validate-scene",
        if cfg.validate_scene == 0 {
            "off".into()
        } else {
            cfg.validate_scene.to_string()
        },
    );
    value("--whisper-cmd", cfg.whisper_cmd.clone());
    value("--whisper-model", cfg.whisper_model.clone());
    value(
        "--caption-style",
        format!("{:?}", cfg.caption_style).to_lowercase(),
    );
    if let Some(font) = &cfg.caption_font {
        value("--caption-font", font.clone());
    }
    value("--scene-seconds", cfg.scene_seconds.to_string());
    value("--dissolve-seconds", cli.dissolve_seconds.to_string());
    value("--music-volume", cli.music_volume.to_string());
    value("--mix", format!("{:?}", cli.mix).to_lowercase());
    value(
        "--export-preset",
        format!("{:?}", cli.export_preset).to_lowercase(),
    );
    if let Some(scene) = cli.poster_scene {
        value("--poster-scene", scene.to_string());
    }
    for id in &cli.video_scene_ids {
        value("--video-scene-id", id.clone());
    }
    for id in &cli.regenerate_image_scene_ids {
        value("--regenerate-image-scene-id", id.clone());
    }
    for id in &cli.keep_image_scene_ids {
        value("--keep-image-scene-id", id.clone());
    }
    for id in &cli.regenerate_video_scene_ids {
        value("--regenerate-video-scene-id", id.clone());
    }
    for id in &cli.keep_video_scene_ids {
        value("--keep-video-scene-id", id.clone());
    }
    if let Some(action) = cli.music_action {
        value("--music-action", format!("{action:?}").to_lowercase());
    }
    if let Some(path) = &cli.music {
        value("--music", path.display().to_string());
    }
    if let Some(path) = &cli.character_ref {
        value("--character-ref", path.display().to_string());
    }
    if let Some(path) = &cli.watermark {
        value("--watermark", path.display().to_string());
    }
    if let Some(max) = cfg.max_cost {
        value("--max-cost", max.to_string());
    }
    args
}

fn validate_revision_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 96
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        bail!("revision_id must be 1-96 ASCII letters, digits, hyphens, or underscores");
    }
    Ok(())
}

fn validate_script(script: &Script) -> Result<()> {
    if script.scenes.is_empty() || script.scenes.iter().any(|s| s.id.trim().is_empty()) {
        bail!("revision script requires at least one scene with a stable id");
    }
    let ids: BTreeSet<&str> = script.scenes.iter().map(|s| s.id.as_str()).collect();
    if ids.len() != script.scenes.len() {
        bail!("revision script contains duplicate scene ids");
    }
    if script
        .scenes
        .iter()
        .any(|scene| scene.line.trim().is_empty())
    {
        bail!("every revision scene must own a non-empty narration line");
    }
    let lines = normalize_words(
        &script
            .scenes
            .iter()
            .map(|s| s.line.as_str())
            .collect::<Vec<_>>()
            .join(" "),
    );
    if lines != normalize_words(&script.narration) {
        bail!("scene lines must cover the complete narration in order");
    }
    let mut next_scene = 0;
    for chapter in &script.chapters {
        if chapter.scene_count == 0
            || chapter.scene_start != next_scene
            || chapter.scene_start + chapter.scene_count > script.scenes.len()
        {
            bail!("chapter scene range is invalid");
        }
        let chapter_lines = script.scenes
            [chapter.scene_start..chapter.scene_start + chapter.scene_count]
            .iter()
            .map(|scene| scene.line.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        if normalize_words(&chapter_lines) != normalize_words(&chapter.narration) {
            bail!("chapter narration must match the lines owned by its scene range");
        }
        next_scene += chapter.scene_count;
    }
    if !script.chapters.is_empty() {
        if next_scene != script.scenes.len() {
            bail!("chapter ranges must cover every scene exactly once");
        }
        let narration = script
            .chapters
            .iter()
            .map(|c| c.narration.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        if normalize_words(&narration) != normalize_words(&script.narration) {
            bail!("chapter narration must cover the complete narration in order");
        }
    }
    Ok(())
}

fn normalize_words(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn validate_scene_flags(cli: &Cli, script: &Script) -> Result<()> {
    let valid: BTreeSet<&str> = script.scenes.iter().map(|s| s.id.as_str()).collect();
    for (name, ids) in [
        ("video-scene-id", &cli.video_scene_ids),
        ("regenerate-image-scene-id", &cli.regenerate_image_scene_ids),
        ("keep-image-scene-id", &cli.keep_image_scene_ids),
        ("regenerate-video-scene-id", &cli.regenerate_video_scene_ids),
        ("keep-video-scene-id", &cli.keep_video_scene_ids),
    ] {
        let unique: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
        if unique.len() != ids.len() {
            bail!("--{name} contains a duplicate stable scene id");
        }
        if let Some(id) = unique.iter().find(|id| !valid.contains(**id)) {
            bail!("--{name} references unknown scene id {id:?}");
        }
    }
    conflict(
        &cli.regenerate_image_scene_ids,
        &cli.keep_image_scene_ids,
        "image",
    )?;
    conflict(
        &cli.regenerate_video_scene_ids,
        &cli.keep_video_scene_ids,
        "video",
    )?;
    let selected: BTreeSet<&str> = cli.video_scene_ids.iter().map(String::as_str).collect();
    for id in cli
        .regenerate_video_scene_ids
        .iter()
        .chain(&cli.keep_video_scene_ids)
    {
        if !selected.contains(id.as_str()) {
            bail!("video keep/regenerate id {id:?} must also be selected with --video-scene-id");
        }
    }
    Ok(())
}

fn conflict(a: &[String], b: &[String], label: &str) -> Result<()> {
    let b: BTreeSet<&str> = b.iter().map(String::as_str).collect();
    if let Some(id) = a.iter().find(|id| b.contains(id.as_str())) {
        bail!("scene {id:?} cannot both keep and regenerate its {label}");
    }
    Ok(())
}

fn build_actions(
    source: &Path,
    prior: &Script,
    desired: &Script,
    cli: &Cli,
    cfg: &Config,
    manifest: Option<&RunManifest>,
) -> Result<(Vec<ArtifactAction>, PlanCost, Vec<String>)> {
    let mut actions = Vec::new();
    let prior_by_id: HashMap<&str, (usize, &Scene)> = prior
        .scenes
        .iter()
        .enumerate()
        .map(|(index, scene)| (scene.id.as_str(), (index, scene)))
        .collect();
    let current_config = config_value(cli, cfg);
    let previous_config = manifest.map(|m| &m.config);
    let spoken_changed = prior.narration != desired.narration
        || prior.narrator_gender != desired.narrator_gender
        || previous_config.is_some_and(|old| {
            old["voice"] != current_config["voice"]
                || old["tts_model"] != current_config["tts_model"]
                || old["speed"] != json!(cli.speed)
                || old["no_narration"] != json!(cfg.no_narration)
                || (cfg.no_narration && old["scene_seconds"] != json!(cfg.scene_seconds))
        });
    let geometry_changed = prior.format != desired.format
        || previous_config.is_some_and(|old| old["format"] != current_config["format"]);
    let image_config_changed = geometry_changed
        || (manifest.is_none() && cli.character_ref.is_some())
        || previous_config.is_some_and(|old| {
            ["image_model", "validate_scene", "consistency"]
                .iter()
                .any(|key| old[*key] != current_config[*key])
                || old["character_ref_sha256"] != current_config["character_ref_sha256"]
        });
    let video_config_changed = previous_config.is_some_and(|old| {
        [
            "video_model",
            "video_provider",
            "video_input_mode",
            "video_seed",
            "video_steps",
            "video_resolution",
            "format",
            "no_narration",
            "scene_seconds",
        ]
        .iter()
        .any(|key| old[*key] != current_config[*key])
    });
    actions.push(action(
        "script",
        ActionKind::Derive,
        "edited_revision",
        None,
        Some("script.json"),
        btree([("content", hash_json(desired)?)]),
    ));
    let audio_kind = if !spoken_changed && source.join("audio.mp3").is_file() {
        ActionKind::Reuse
    } else {
        ActionKind::Generate
    };
    actions.push(action(
        "narration",
        audio_kind.clone(),
        if spoken_changed {
            "spoken_inputs_changed"
        } else {
            "dependencies_match"
        },
        audio_kind.eq(&ActionKind::Reuse).then_some("audio.mp3"),
        Some("audio.mp3"),
        if audio_kind == ActionKind::Reuse {
            source_dependencies(manifest, "narration")
        } else {
            spoken_dependencies(desired, cli, cfg)
        },
    ));
    let words_kind = if audio_kind == ActionKind::Reuse && source.join("words.json").is_file() {
        ActionKind::Reuse
    } else {
        ActionKind::Derive
    };
    actions.push(action(
        "timings",
        words_kind.clone(),
        if words_kind == ActionKind::Reuse {
            "narration_unchanged"
        } else {
            "narration_changed"
        },
        words_kind.eq(&ActionKind::Reuse).then_some("words.json"),
        Some("words.json"),
        if words_kind == ActionKind::Reuse {
            source_dependencies(manifest, "timings")
        } else {
            btree([(
                "narration",
                hash_json(&spoken_dependencies(desired, cli, cfg))?,
            )])
        },
    ));

    let force_images: BTreeSet<&str> = cli
        .regenerate_image_scene_ids
        .iter()
        .map(String::as_str)
        .collect();
    let keep_images: BTreeSet<&str> = cli
        .keep_image_scene_ids
        .iter()
        .map(String::as_str)
        .collect();
    let force_clips: BTreeSet<&str> = cli
        .regenerate_video_scene_ids
        .iter()
        .map(String::as_str)
        .collect();
    let keep_clips: BTreeSet<&str> = cli
        .keep_video_scene_ids
        .iter()
        .map(String::as_str)
        .collect();
    let selected: BTreeSet<&str> = if !cli.video_scene_ids.is_empty() {
        cli.video_scene_ids.iter().map(String::as_str).collect()
    } else {
        let count = cli
            .video_scenes
            .unwrap_or(if cli.video { desired.scenes.len() } else { 0 });
        desired
            .scenes
            .iter()
            .take(count)
            .map(|s| s.id.as_str())
            .collect()
    };
    let mut generated_images = 0;
    let mut generated_video = 0;
    for (new_index, scene) in desired.scenes.iter().enumerate() {
        let still_artifact = format!("scene:{}:still", scene.id);
        let old = prior_by_id.get(scene.id.as_str()).copied();
        let old_still = old.map(|(i, _)| format!("scene-{i:02}.jpg"));
        let still_exists = old_still.as_ref().is_some_and(|p| source.join(p).is_file());
        let desired_image_input = image_inputs(scene, desired);
        let recorded_image_input =
            manifest.and_then(|manifest| recorded_dependency(manifest, &still_artifact, "input"));
        let image_changed = image_config_changed
            || recorded_image_input.is_some_and(|input| input != &desired_image_input)
            || old.is_none_or(|(_, old)| image_inputs(old, prior) != image_inputs(scene, desired));
        let (kind, reason) = if keep_images.contains(scene.id.as_str()) {
            if !still_exists {
                bail!("cannot keep missing source still for {}", scene.id);
            }
            (ActionKind::Reuse, "explicit_keep")
        } else if force_images.contains(scene.id.as_str()) {
            (ActionKind::Generate, "explicit_regenerate")
        } else if still_exists && !image_changed {
            (ActionKind::Reuse, "dependencies_match")
        } else {
            (
                ActionKind::Generate,
                if image_changed {
                    "image_inputs_changed"
                } else {
                    "source_missing"
                },
            )
        };
        generated_images += usize::from(kind == ActionKind::Generate);
        let current_still_deps = btree([("input", desired_image_input)]);
        let still_deps = if kind == ActionKind::Reuse {
            source_dependencies(manifest, &still_artifact)
        } else {
            current_still_deps
        };
        actions.push(action(
            &still_artifact,
            kind.clone(),
            reason,
            (kind == ActionKind::Reuse)
                .then_some(old_still.as_deref())
                .flatten(),
            Some(&format!("scene-{new_index:02}.jpg")),
            still_deps,
        ));

        let clip_artifact = format!("scene:{}:video", scene.id);
        let old_clip = old.map(|(i, _)| format!("scene-{i:02}.mp4"));
        let clip_exists = old_clip.as_ref().is_some_and(|p| source.join(p).is_file());
        let desired_video_input = video_inputs(scene, desired, cfg);
        let recorded_video_input =
            manifest.and_then(|manifest| recorded_dependency(manifest, &clip_artifact, "input"));
        let clip_changed = video_config_changed
            || recorded_video_input.is_some_and(|input| input != &desired_video_input)
            || old.is_none_or(|(_, old)| {
                video_inputs(old, prior, cfg) != video_inputs(scene, desired, cfg)
            })
            || (cfg.video_input_mode == VideoInputMode::FirstFrame && kind != ActionKind::Reuse);
        let (clip_kind, clip_reason) = if !selected.contains(scene.id.as_str()) {
            (ActionKind::Exclude, "not_selected")
        } else if keep_clips.contains(scene.id.as_str()) {
            if !clip_exists {
                bail!("cannot keep missing source clip for {}", scene.id);
            }
            (ActionKind::Reuse, "explicit_keep")
        } else if force_clips.contains(scene.id.as_str()) {
            (ActionKind::Generate, "explicit_regenerate")
        } else if clip_exists && !clip_changed {
            (ActionKind::Reuse, "dependencies_match")
        } else {
            (
                ActionKind::Generate,
                if clip_changed {
                    "video_inputs_changed"
                } else {
                    "source_missing"
                },
            )
        };
        generated_video += usize::from(clip_kind == ActionKind::Generate);
        let current_clip_deps = btree([("input", desired_video_input)]);
        let clip_deps = if clip_kind == ActionKind::Reuse {
            source_dependencies(manifest, &clip_artifact)
        } else {
            current_clip_deps
        };
        actions.push(action(
            &clip_artifact,
            clip_kind.clone(),
            clip_reason,
            (clip_kind == ActionKind::Reuse)
                .then_some(old_clip.as_deref())
                .flatten(),
            (clip_kind != ActionKind::Exclude)
                .then(|| format!("scene-{new_index:02}.mp4"))
                .as_deref(),
            clip_deps,
        ));
        if clip_kind == ActionKind::Generate {
            if let Some((old_index, _)) = old {
                let job = format!("scene-{old_index:02}.video-job.json");
                if source.join(&job).is_file()
                    && !clip_changed
                    && !force_clips.contains(scene.id.as_str())
                {
                    actions.push(action(
                        &format!("scene:{}:provider_job", scene.id),
                        ActionKind::Reuse,
                        "accepted_job_resume",
                        Some(&job),
                        Some(&format!("scene-{new_index:02}.video-job.json")),
                        BTreeMap::new(),
                    ));
                }
            }
        }
    }
    let needs_references = generated_images > 0 && (cli.consistency || !cli.no_consistency);
    for entity in &desired.characters {
        let unchanged = !image_config_changed && prior.characters.iter().any(|old| old == entity);
        for name in [
            format!("character-{}.jpg", crate::images::slug(&entity.id)),
            format!("character-{}-b.jpg", crate::images::slug(&entity.id)),
        ] {
            let reuse = unchanged && source.join(&name).is_file();
            let kind = if reuse {
                ActionKind::Reuse
            } else if needs_references {
                generated_images += 1;
                ActionKind::Generate
            } else {
                ActionKind::Exclude
            };
            actions.push(action(
                &format!("reference:{name}"),
                kind.clone(),
                if reuse {
                    "entity_inputs_match"
                } else {
                    "entity_inputs_changed"
                },
                reuse.then_some(name.as_str()),
                (kind != ActionKind::Exclude).then_some(name.as_str()),
                if reuse {
                    source_dependencies(manifest, &format!("reference:{name}"))
                } else {
                    btree([
                        ("entity", hash_json(entity)?),
                        ("image_config", hash_json(&current_config)?),
                    ])
                },
            ));
        }
    }
    for entity in &desired.locations {
        let unchanged = !image_config_changed && prior.locations.iter().any(|old| old == entity);
        let name = format!("location-{}.jpg", crate::images::slug(&entity.id));
        let reuse = unchanged && source.join(&name).is_file();
        let kind = if reuse {
            ActionKind::Reuse
        } else if needs_references {
            generated_images += 1;
            ActionKind::Generate
        } else {
            ActionKind::Exclude
        };
        actions.push(action(
            &format!("reference:{name}"),
            kind.clone(),
            if reuse {
                "entity_inputs_match"
            } else {
                "entity_inputs_changed"
            },
            reuse.then_some(name.as_str()),
            (kind != ActionKind::Exclude).then_some(name.as_str()),
            if reuse {
                source_dependencies(manifest, &format!("reference:{name}"))
            } else {
                btree([
                    ("entity", hash_json(entity)?),
                    ("image_config", hash_json(&current_config)?),
                ])
            },
        ));
    }
    if prior.characters.first() == desired.characters.first()
        && source.join("character-ref.jpg").is_file()
    {
        let reuse = !image_config_changed;
        actions.push(action(
            "reference:character-ref.jpg",
            if reuse {
                ActionKind::Reuse
            } else {
                ActionKind::Derive
            },
            if reuse {
                "primary_entity_matches"
            } else {
                "reference_input_changed"
            },
            reuse.then_some("character-ref.jpg"),
            Some("character-ref.jpg"),
            if reuse {
                source_dependencies(manifest, "reference:character-ref.jpg")
            } else {
                btree([(
                    "input",
                    current_config["character_ref_sha256"]
                        .as_str()
                        .unwrap_or("generated")
                        .to_owned(),
                )])
            },
        ));
    }
    let music_file = existing_music_name(source);
    let music_action = cli.music_action.unwrap_or(if cli.music_gen {
        MusicAction::Regenerate
    } else {
        MusicAction::Keep
    });
    let (music_kind, music_reason) = match music_action {
        MusicAction::Remove => (ActionKind::Exclude, "explicit_remove"),
        MusicAction::Regenerate => (ActionKind::Generate, "explicit_regenerate"),
        MusicAction::Keep if music_file.is_some() => (ActionKind::Reuse, "explicit_keep"),
        MusicAction::Keep => (ActionKind::Exclude, "source_missing"),
    };
    actions.push(action(
        "music",
        music_kind.clone(),
        music_reason,
        music_file.as_deref(),
        music_file
            .as_deref()
            .or((music_kind == ActionKind::Generate).then_some("music.wav")),
        if music_kind == ActionKind::Reuse {
            source_dependencies(manifest, "music")
        } else {
            btree([("prompt", hash_bytes(desired.music_prompt.as_bytes()))])
        },
    ));
    let poster_changed = prior.poster_prompt != desired.poster_prompt
        || prior.characters.first() != desired.characters.first()
        || image_config_changed;
    let poster_kind = if source.join("poster.jpg").is_file() && !poster_changed {
        ActionKind::Reuse
    } else {
        ActionKind::Generate
    };
    generated_images += usize::from(poster_kind == ActionKind::Generate);
    actions.push(action(
        "poster",
        poster_kind.clone(),
        if poster_changed {
            "poster_inputs_changed"
        } else if poster_kind == ActionKind::Reuse {
            "dependencies_match"
        } else {
            "source_missing"
        },
        (poster_kind == ActionKind::Reuse).then_some("poster.jpg"),
        Some("poster.jpg"),
        if poster_kind == ActionKind::Reuse {
            source_dependencies(manifest, "poster")
        } else {
            btree([(
                "input",
                hash_json(&json!({
                    "prompt": desired.poster_prompt,
                    "character": desired.characters.first(),
                    "format": desired.format,
                    "image_model": cfg.image_model,
                    "character_ref_sha256": current_config["character_ref_sha256"],
                }))?,
            )])
        },
    ));
    actions.push(action(
        "output",
        ActionKind::Derive,
        "revision_render",
        None,
        Some(if selected.is_empty() {
            "reel.mp4"
        } else {
            "reel-video.mp4"
        }),
        btree([("config", hash_json(&config_value(cli, cfg))?)]),
    ));

    let narration = if audio_kind == ActionKind::Generate && !cfg.no_narration {
        config::tts_cost(desired.narration.split_whitespace().count())
    } else {
        0.0
    };
    let images = config::image_cost(&cfg.image_model)
        * generated_images as f64
        * cfg.validate_scene.max(1) as f64;
    let video_seconds = generated_video as f64 * 8.0;
    let video = if cfg.video_provider == VideoProvider::Local {
        0.0
    } else {
        config::video_cost_per_second(&cfg.video_model, &cfg.video_resolution) * video_seconds
    };
    let music = if music_kind == ActionKind::Generate {
        config::music_cost()
    } else {
        0.0
    };
    let total = narration + images + video + music;
    let warnings = manifest.is_none().then(|| "legacy source provenance explicitly trusted; retained assets remain marked as legacy/unknown".to_string()).into_iter().collect();
    Ok((
        actions,
        PlanCost {
            total_usd: total,
            script_usd: 0.0,
            narration_usd: narration,
            images_usd: images,
            video_usd: video,
            music_usd: music,
            uncertain: generated_video > 0,
        },
        warnings,
    ))
}

fn action(
    artifact: &str,
    action: ActionKind,
    reason: &str,
    source: Option<&str>,
    destination: Option<&str>,
    dependencies: BTreeMap<String, String>,
) -> ArtifactAction {
    ArtifactAction {
        artifact: artifact.into(),
        action,
        reason: reason.into(),
        source: source.map(str::to_owned),
        destination: destination.map(str::to_owned),
        dependencies,
    }
}

fn btree<const N: usize>(pairs: [(&str, String); N]) -> BTreeMap<String, String> {
    pairs.into_iter().map(|(k, v)| (k.into(), v)).collect()
}

fn spoken_dependencies(script: &Script, cli: &Cli, cfg: &Config) -> BTreeMap<String, String> {
    btree([
        ("narration", hash_bytes(script.narration.as_bytes())),
        (
            "narrator_gender",
            hash_bytes(script.narrator_gender.as_bytes()),
        ),
        ("voice", hash_json(&cfg.voice).unwrap()),
        ("tts_model", hash_bytes(cfg.tts_model.as_bytes())),
        ("speed", hash_json(&cli.speed).unwrap()),
        ("no_narration", hash_json(&cfg.no_narration).unwrap()),
        ("scene_seconds", hash_json(&cfg.scene_seconds).unwrap()),
    ])
}

fn source_dependencies(manifest: Option<&RunManifest>, artifact: &str) -> BTreeMap<String, String> {
    manifest
        .and_then(|manifest| {
            manifest
                .actions
                .iter()
                .find(|action| action.artifact == artifact)
                .map(|action| action.dependencies.clone())
        })
        .unwrap_or_else(|| btree([("provenance", "legacy:unknown".into())]))
}

fn recorded_dependency<'a>(
    manifest: &'a RunManifest,
    artifact: &str,
    key: &str,
) -> Option<&'a String> {
    manifest
        .actions
        .iter()
        .find(|action| action.artifact == artifact)?
        .dependencies
        .get(key)
}

fn image_inputs(scene: &Scene, script: &Script) -> String {
    let characters: Vec<_> = script
        .characters
        .iter()
        .filter(|entity| scene.cast_ids.contains(&entity.id))
        .collect();
    let location = script
        .locations
        .iter()
        .find(|entity| entity.id == scene.location_id);
    hash_json(&json!({
        "prompt":scene.image_prompt,"cast_ids":scene.cast_ids,"location_id":scene.location_id,
        "characters":characters,"location":location,"format":script.format
    }))
    .unwrap()
}

fn video_inputs(scene: &Scene, script: &Script, cfg: &Config) -> String {
    let visual_input = match cfg.video_input_mode {
        VideoInputMode::FirstFrame => image_inputs(scene, script),
        VideoInputMode::Text => hash_json(&json!({
            "image_prompt":scene.image_prompt,
            "cast_ids":scene.cast_ids,
            "location_id":scene.location_id,
            "characters":script.characters.iter().filter(|e| scene.cast_ids.contains(&e.id)).collect::<Vec<_>>(),
            "location":script.locations.iter().find(|e| e.id == scene.location_id),
        }))
        .unwrap(),
    };
    hash_json(&json!({
        "visual_input":visual_input,"line":scene.line,"motion":scene.motion_prompt,
        "timeline_narration":script.narration,"provider":format!("{:?}",cfg.video_provider),
        "model":cfg.video_model,"resolution":cfg.video_resolution,
        "input_mode":format!("{:?}",cfg.video_input_mode),"seed":cfg.video_seed,
        "steps":cfg.video_steps,"no_narration":cfg.no_narration,
        "scene_seconds":cfg.scene_seconds
    }))
    .unwrap()
}

fn config_value(cli: &Cli, cfg: &Config) -> Value {
    json!({
        "format": format!("{:?}", cfg.format).to_lowercase(), "minutes":cfg.minutes,
        "text_model":cfg.text_model,"image_model":cfg.image_model,"judge_model":cfg.judge_model,
        "tts_model":cfg.tts_model,"music_model":cfg.music_model,"video_model":cfg.video_model,
        "video_provider":format!("{:?}",cfg.video_provider).to_lowercase(),"video_base_url":cfg.video_base_url,
        "video_input_mode":format!("{:?}",cfg.video_input_mode).to_lowercase(),"video_seed":cfg.video_seed,
        "video_steps":cfg.video_steps,"video_wait_timeout":cfg.video_wait_timeout,
        "video_resolution":cfg.video_resolution,"voice":cfg.voice,
        "speed":cli.speed,"validate_scene":cfg.validate_scene,"no_narration":cfg.no_narration,
        "scene_seconds":cfg.scene_seconds,"no_captions":cfg.no_captions,"caption_style":format!("{:?}",cfg.caption_style).to_lowercase(),
        "caption_font":cfg.caption_font,"whisper_cmd":cfg.whisper_cmd,"whisper_model":cfg.whisper_model,
        "character_ref":canonical_input(cli.character_ref.as_deref()),"character_ref_sha256":cli.character_ref.as_deref().and_then(|path| hash_file(path).ok()),
        "watermark":canonical_input(cli.watermark.as_deref()),"watermark_sha256":cli.watermark.as_deref().and_then(|path| hash_file(path).ok()),
        "mix":format!("{:?}",cli.mix).to_lowercase(),"music_volume":cli.music_volume,
        "dissolve":cli.dissolve || !cli.no_dissolve,"dissolve_seconds":cli.dissolve_seconds,"grade":cli.grade || !cli.no_grade,
        "loudnorm":cli.loudnorm || !cli.no_loudnorm,"poster_scene":cli.poster_scene,"embed_poster":cli.embed_poster || !cli.no_embed_poster,
        "consistency":cli.consistency || !cli.no_consistency,"export_preset":format!("{:?}",cli.export_preset).to_lowercase()
    })
}

pub fn execute(path: &Path, approval: Option<&str>, recovering: bool) -> Result<()> {
    let plan: RevisionPlan =
        serde_json::from_slice(&fs::read(path)?).context("invalid approved Plan v1 JSON")?;
    let mut events = Events::new(&plan);
    if plan_hash(&plan)? != plan.approval_hash {
        events.emit(
            "planning",
            "failed",
            None,
            Some("plan_tampered"),
            Some("the persisted plan content does not match its approval hash"),
        );
        bail!("plan_tampered");
    }
    let approval = match approval {
        Some(approval) => approval,
        None => {
            events.emit(
                "planning",
                "failed",
                None,
                Some("approval_hash_missing"),
                Some("--approval-hash is required"),
            );
            bail!("approval_hash_missing");
        }
    };
    if approval != plan.approval_hash {
        events.emit(
            "planning",
            "failed",
            None,
            Some("approval_hash_mismatch"),
            Some("the supplied approval hash does not match the persisted plan"),
        );
        bail!("approval_hash_mismatch");
    }
    match execute_inner(&plan, recovering, &mut events) {
        Ok(()) => Ok(()),
        Err(error) => {
            let text = format!("{error:#}");
            let code = if text.contains("accepted_provider_job_pending") {
                "accepted_provider_job_pending"
            } else if text.contains("ambiguous_paid_work") {
                "ambiguous_paid_work"
            } else if text.contains("stale_plan") {
                "stale_plan"
            } else {
                "revision_execution_failed"
            };
            events.emit("publish", "failed", None, Some(code), Some(&text));
            if matches!(
                code,
                "accepted_provider_job_pending" | "ambiguous_paid_work"
            ) {
                std::process::exit(75);
            }
            Err(error)
        }
    }
}

fn execute_inner(plan: &RevisionPlan, _recovering: bool, events: &mut Events<'_>) -> Result<()> {
    events.emit("planning", "started", None, None, None);
    reject_symlink_dir(&plan.source, "source")?;
    let _source_lock = lock_run(&plan.source)?;
    let current =
        make_plan(plan.request.clone()).context("stale_plan: could not reproduce approved plan")?;
    if current != *plan {
        bail!("stale_plan: source artifacts or resolved configuration changed; request a new plan");
    }
    events.emit("planning", "completed", None, None, None);
    let revisions = plan.destination.parent().unwrap();
    reject_symlink_if_exists(revisions, "revisions directory")?;
    fs::create_dir_all(revisions.join(".locks"))?;
    reject_symlink_if_exists(&revisions.join(".locks"), "revision lock directory")?;
    let lock_path = revisions
        .join(".locks")
        .join(format!("{}.lock", plan.request.revision_id));
    reject_symlink_if_exists(&lock_path, "revision lock file")?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)?;
    lock.try_lock_exclusive()
        .context("revision_locked: another worker owns this revision")?;
    events.emit("locking", "completed", None, None, None);
    reject_symlink_if_exists(&plan.destination, "revision destination")?;
    if plan.destination.exists() {
        let manifest = read_manifest(&plan.destination)?
            .ok_or_else(|| anyhow!("immutable destination exists without a completed manifest"))?;
        if manifest.state == "completed" && manifest.approval_hash == plan.approval_hash {
            events.emit(
                "publish",
                "completed",
                Some(plan.destination.display().to_string()),
                None,
                None,
            );
            return Ok(());
        }
        bail!("immutable destination already exists with a different approval");
    }
    let work = revisions.join(format!(".{}.work", plan.request.revision_id));
    reject_symlink_if_exists(&work, "revision work directory")?;
    if work.exists() {
        if let Some(state) = read_execution_state(&work)? {
            if state.approval_hash != plan.approval_hash {
                bail!("draft approval does not match the approved plan");
            }
            if state.state == "running" && has_ambiguous_paid_work(plan, &work) {
                bail!("ambiguous_paid_work: interrupted hosted generation has no durable provider ID; create and approve a new revision to replace it explicitly");
            }
        }
    } else {
        fs::create_dir(&work)?;
    }
    events.emit("copying", "started", None, None, None);
    for item in &plan.actions {
        if item.action == ActionKind::Reuse {
            if let (Some(from), Some(to)) = (&item.source, &item.destination) {
                let expected = plan
                    .source_snapshot
                    .get(from)
                    .ok_or_else(|| anyhow!("approved source snapshot has no hash for {from}"))?;
                copy_atomic_verified(&plan.source.join(from), &work.join(to), expected)?;
            }
        }
    }
    atomic_json(
        &work.join("script.json"),
        plan.request.script.as_ref().unwrap(),
    )?;
    atomic_json(&work.join(PLAN_COPY), plan)?;
    write_execution_state(&work, plan, "prepared")?;
    events.emit("copying", "completed", None, None, None);

    let mut args = vec![
        "--no-dotenv".to_string(),
        "--from".to_string(),
        work.display().to_string(),
    ];
    args.extend(plan.request.arguments.iter().cloned());
    write_execution_state(&work, plan, "running")?;
    let (status, _) = run_child(&args, events)?;
    if !status.success() {
        if has_ambiguous_paid_work(plan, &work) {
            bail!("ambiguous_paid_work: pipeline stopped while hosted paid work lacked a durable completion record");
        }
        write_execution_state(&work, plan, "interrupted_local")?;
        bail!("pipeline child exited with {status}");
    }
    let pending = pending_accepted_jobs(&work)?;
    if !pending.is_empty() {
        write_execution_state(&work, plan, "awaiting_provider")?;
        bail!("accepted_provider_job_pending: {}", pending.join(", "));
    }
    let planned_output = plan
        .actions
        .iter()
        .find(|a| a.artifact == "output")
        .and_then(|a| a.destination.as_deref())
        .unwrap_or("reel.mp4");
    let output_name = if work.join(planned_output).is_file() {
        planned_output
    } else if planned_output == "reel-video.mp4" && work.join("reel.mp4").is_file() {
        let warning =
            "requested video clips were unavailable; published the validated still fallback";
        events.emit(
            "video",
            "warning",
            Some("reel.mp4".into()),
            Some("requested_video_fallback"),
            Some(warning),
        );
        "reel.mp4"
    } else {
        planned_output
    };
    let output = work.join(output_name);
    crate::ffmpeg::validate_video(&output)
        .with_context(|| format!("completed_output_invalid: {output_name}"))?;
    let mut warnings = plan.warnings.clone();
    for action in &plan.actions {
        if action.artifact.ends_with(":video") && action.action == ActionKind::Generate {
            if let Some(name) = &action.destination {
                if !work.join(name).is_file() {
                    let warning = format!("requested clip unavailable: {}", action.artifact);
                    warnings.push(warning.clone());
                    events.emit(
                        "video",
                        "warning",
                        None,
                        Some("requested_clip_unavailable"),
                        Some(&warning),
                    );
                }
            }
        }
    }
    let artifacts = snapshot(&work)?;
    let manifest = RunManifest {
        version: 1,
        state: "completed".into(),
        run_id: plan
            .destination
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .and_then(|s| s.to_str())
            .unwrap_or("run")
            .into(),
        revision_id: plan.request.revision_id.clone(),
        parent_revision: plan.parent_revision.clone(),
        approval_hash: plan.approval_hash.clone(),
        source: plan.source.clone(),
        config: plan.resolved_config.clone(),
        artifacts,
        actions: plan.actions.clone(),
        warnings: warnings.clone(),
        outputs: discover_outputs(&work)?,
    };
    write_execution_state(&work, plan, "completed")?;
    atomic_json(&work.join(MANIFEST), &manifest)?;
    File::open(&work)?.sync_all()?;
    fs::rename(&work, &plan.destination)?;
    File::open(revisions)?.sync_all()?;
    for warning in &warnings {
        events.emit(
            "publish",
            "warning",
            None,
            Some("completed_with_warning"),
            Some(warning),
        );
    }
    events.emit(
        "publish",
        "completed",
        Some(plan.destination.join(output_name).display().to_string()),
        None,
        None,
    );
    Ok(())
}

pub fn execute_fresh_events() -> Result<()> {
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|arg| arg != "--events-json")
        .collect();
    let identity = json!({"run_id":"fresh","revision_id":"fresh","job_id":""});
    let mut events = Events::anonymous(identity);
    let (status, artifact) = run_child(&args, &mut events)?;
    if status.success() {
        let validated = (|| -> Result<String> {
            let artifact = artifact.ok_or_else(|| {
                anyhow!("completed_output_missing: successful pipeline did not report an artifact")
            })?;
            if !args.iter().any(|arg| arg == "--no-images") {
                crate::ffmpeg::validate_video(Path::new(&artifact))
                    .context("completed_output_invalid")?;
            } else if !Path::new(&artifact).is_file() {
                bail!("completed_output_missing: {artifact}");
            }
            Ok(artifact)
        })();
        match validated {
            Ok(artifact) => {
                events.emit("publish", "completed", Some(artifact), None, None);
                Ok(())
            }
            Err(error) => {
                let text = format!("{error:#}");
                events.emit(
                    "publish",
                    "failed",
                    None,
                    Some("completed_output_invalid"),
                    Some(&text),
                );
                Err(error)
            }
        }
    } else {
        events.emit(
            "publish",
            "failed",
            None,
            Some("pipeline_failed"),
            Some(&status.to_string()),
        );
        bail!("pipeline child exited with {status}")
    }
}

fn run_child(
    args: &[String],
    events: &mut Events<'_>,
) -> Result<(std::process::ExitStatus, Option<String>)> {
    let mut child = Command::new(std::env::current_exe()?)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    for (is_stderr, reader) in [
        (
            false,
            Box::new(BufReader::new(stdout)) as Box<dyn BufRead + Send>,
        ),
        (
            true,
            Box::new(BufReader::new(stderr)) as Box<dyn BufRead + Send>,
        ),
    ] {
        let sender = sender.clone();
        std::thread::spawn(move || {
            for line in reader.lines().map_while(Result::ok) {
                let _ = sender.send((is_stderr, line));
            }
        });
    }
    drop(sender);
    let mut previous = None;
    let mut artifact = None;
    for (is_stderr, line) in receiver {
        eprintln!("{line}");
        if is_stderr
            && (line.to_ascii_lowercase().contains("warning:")
                || line.trim_start().starts_with("note:"))
        {
            events.emit(
                previous.unwrap_or("pipeline"),
                "warning",
                None,
                Some("pipeline_warning"),
                Some(line.trim()),
            );
        }
        if let Some(path) = line
            .split_once("✓ done: ")
            .map(|(_, path)| path.trim().to_string())
        {
            artifact = Some(path);
        } else if let Some(path) = line
            .split_once("word timings written to ")
            .map(|(_, path)| path.trim().to_string())
        {
            artifact = Some(path);
        }
        if let Some(stage) = stage_for_line(&line) {
            if previous != Some(stage) {
                if let Some(done) = previous {
                    events.emit(done, "completed", None, None, None);
                }
                events.emit(stage, "started", None, None, None);
                previous = Some(stage);
            }
        }
    }
    let status = child.wait()?;
    if let Some(done) = previous {
        events.emit(done, "completed", None, None, None);
    }
    Ok((status, artifact))
}

fn stage_for_line(line: &str) -> Option<&'static str> {
    if line.contains("writing script") {
        Some("script")
    } else if line.contains("synthesizing") {
        Some("narration")
    } else if line.contains("timing narration") {
        Some("timing")
    } else if line.contains("generating") && line.contains("image") {
        Some("images")
    } else if line.contains("video scene") {
        Some("video")
    } else if line.contains("soundtrack") {
        Some("music")
    } else if line.contains("assembling") {
        Some("render")
    } else if line.contains("poster") {
        Some("poster")
    } else {
        None
    }
}

struct Events<'a> {
    sequence: u64,
    started: Instant,
    run_id: String,
    revision_id: String,
    job_id: String,
    _marker: std::marker::PhantomData<&'a ()>,
}
impl<'a> Events<'a> {
    fn new(plan: &RevisionPlan) -> Self {
        Self {
            sequence: 0,
            started: Instant::now(),
            run_id: plan
                .destination
                .parent()
                .and_then(Path::parent)
                .and_then(Path::file_name)
                .and_then(|s| s.to_str())
                .unwrap_or("run")
                .into(),
            revision_id: plan.request.revision_id.clone(),
            job_id: plan.request.job_id.clone(),
            _marker: std::marker::PhantomData,
        }
    }
    fn anonymous(v: Value) -> Self {
        Self {
            sequence: 0,
            started: Instant::now(),
            run_id: v["run_id"].as_str().unwrap().into(),
            revision_id: v["revision_id"].as_str().unwrap().into(),
            job_id: v["job_id"].as_str().unwrap().into(),
            _marker: std::marker::PhantomData,
        }
    }
    fn emit(
        &mut self,
        stage: &str,
        state: &str,
        artifact: Option<String>,
        code: Option<&str>,
        message: Option<&str>,
    ) {
        self.sequence += 1;
        println!(
            "{}",
            json!({"version":1,"sequence":self.sequence,"timestamp_unix_ms":now_ms(),"run_id":self.run_id,"revision_id":self.revision_id,"job_id":self.job_id,"stage":stage,"scene_id":Value::Null,"state":state,"elapsed_ms":self.started.elapsed().as_millis() as u64,"artifact":artifact,"code":code,"message":message,"cost":Value::Null})
        );
        let _ = std::io::stdout().flush();
    }
}

fn snapshot(dir: &Path) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for name in top_level_names(dir)? {
        let path = dir.join(&name);
        if path.is_file() && name != PLAN_COPY && !name.starts_with('.') {
            out.insert(name, hash_file(&path)?);
        }
    }
    Ok(out)
}
fn top_level_names(dir: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for e in fs::read_dir(dir)? {
        let e = e?;
        if e.file_type()?.is_file() {
            names.push(e.file_name().to_string_lossy().into_owned());
        }
    }
    names.sort();
    Ok(names)
}
fn hash_file(path: &Path) -> Result<String> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut b = [0u8; 65536];
    loop {
        let n = f.read(&mut b)?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    Ok(format!("sha256:{:x}", h.finalize()))
}
fn hash_bytes(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}
fn hash_json(value: &impl Serialize) -> Result<String> {
    Ok(hash_bytes(&serde_json::to_vec(value)?))
}
fn canonical_input(path: Option<&Path>) -> Option<String> {
    path.map(|path| {
        fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .display()
            .to_string()
    })
}
fn plan_hash(plan: &RevisionPlan) -> Result<String> {
    let mut canonical = plan.clone();
    canonical.approval_hash.clear();
    hash_json(&canonical)
}
fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
fn read_manifest(dir: &Path) -> Result<Option<RunManifest>> {
    match fs::read(dir.join(MANIFEST)) {
        Ok(b) => Ok(Some(
            serde_json::from_slice(&b).context("invalid run-manifest.json")?,
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn existing_music_name(dir: &Path) -> Option<String> {
    ["wav", "mp3", "ogg", "flac"]
        .iter()
        .map(|e| format!("music.{e}"))
        .find(|n| dir.join(n).is_file())
}
fn copy_atomic(from: &Path, to: &Path) -> Result<()> {
    let temp = to.with_extension(format!(
        "{}.copying",
        to.extension().and_then(|s| s.to_str()).unwrap_or("tmp")
    ));
    fs::copy(from, &temp).with_context(|| format!("copying {}", from.display()))?;
    File::open(&temp)?.sync_all()?;
    fs::rename(temp, to)?;
    Ok(())
}
fn copy_atomic_verified(from: &Path, to: &Path, expected: &str) -> Result<()> {
    reject_symlink_if_exists(from, "source artifact")?;
    if hash_file(from)? != expected {
        bail!(
            "stale_plan: source artifact {} changed before copy",
            from.display()
        );
    }
    copy_atomic(from, to)?;
    if hash_file(to)? != expected {
        let _ = fs::remove_file(to);
        bail!("copied artifact hash mismatch for {}", to.display());
    }
    Ok(())
}
fn reject_symlink_dir(path: &Path, label: &str) -> Result<()> {
    reject_symlink_if_exists(path, label)?;
    if !path.is_dir() {
        bail!("{label} must be a directory");
    }
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.'))
    {
        bail!("{label} may not be a hidden work/lock directory");
    }
    Ok(())
}
fn reject_symlink_if_exists(path: &Path, label: &str) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => bail!("{label} may not be a symlink"),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let temp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(value)?;
    let mut f = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temp)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    f.write_all(&bytes)?;
    f.sync_all()?;
    fs::rename(temp, path)?;
    Ok(())
}
fn pending_accepted_jobs(dir: &Path) -> Result<Vec<String>> {
    let mut pending = Vec::new();
    for name in top_level_names(dir)? {
        if !name.ends_with(".video-job.json") {
            continue;
        }
        let v: Value = serde_json::from_slice(&fs::read(dir.join(&name))?)?;
        if v["job_id"].is_string()
            && v["status"] != "completed"
            && !dir.join(name.replace(".video-job.json", ".mp4")).is_file()
        {
            pending.push(format!(
                "{}:{}",
                name,
                v["job_id"].as_str().unwrap_or("unknown")
            ));
        }
    }
    Ok(pending)
}

fn write_execution_state(dir: &Path, plan: &RevisionPlan, state: &str) -> Result<()> {
    atomic_json(
        &dir.join(EXECUTION_STATE),
        &ExecutionState {
            version: 1,
            approval_hash: plan.approval_hash.clone(),
            state: state.into(),
        },
    )
}

fn read_execution_state(dir: &Path) -> Result<Option<ExecutionState>> {
    match fs::read(dir.join(EXECUTION_STATE)) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn has_ambiguous_paid_work(plan: &RevisionPlan, work: &Path) -> bool {
    plan.actions.iter().any(|action| {
        if action.action != ActionKind::Generate {
            return false;
        }
        let missing = action
            .destination
            .as_ref()
            .is_none_or(|destination| !work.join(destination).is_file());
        if !missing {
            return false;
        }
        let idempotent_local_video = action.artifact.ends_with(":video")
            && plan.resolved_config["video_provider"] == "local";
        !idempotent_local_video
    })
}

fn discover_outputs(dir: &Path) -> Result<Vec<OutputArtifact>> {
    let mut outputs = Vec::new();
    for (kind, name) in [
        ("video", "reel-video.mp4"),
        ("video", "reel.mp4"),
        ("poster", "poster.jpg"),
        ("captions", "captions.srt"),
        ("metadata", "metadata.md"),
        ("metadata", "youtube.md"),
        ("narration", "audio.mp3"),
    ] {
        let path = dir.join(name);
        if path.is_file() {
            outputs.push(OutputArtifact {
                kind: kind.into(),
                path: name.into(),
                sha256: hash_file(&path)?,
                bytes: fs::metadata(path)?.len(),
            });
        }
    }
    Ok(outputs)
}

pub fn lock_run(dir: &Path) -> Result<File> {
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join(".reelmaestro.lock"))?;
    lock.try_lock_exclusive()
        .context("run_locked: another Reel Maestro process owns this run")?;
    Ok(lock)
}

pub fn write_native_manifest(dir: &Path, script: &Script, cli: &Cli, cfg: &Config) -> Result<()> {
    let previous = read_manifest(dir)?;
    let mut actions = vec![action(
        "script",
        ActionKind::Derive,
        if cli.from.is_none() {
            "fresh_generation"
        } else {
            "native_resume"
        },
        None,
        Some("script.json"),
        btree([("content", hash_json(script)?)]),
    )];
    if dir.join("audio.mp3").is_file() {
        actions.push(action(
            "narration",
            if cli.from.is_none() {
                ActionKind::Generate
            } else {
                ActionKind::Reuse
            },
            if cli.from.is_none() {
                "fresh_generation"
            } else {
                "native_resume"
            },
            None,
            Some("audio.mp3"),
            if cli.from.is_none() {
                spoken_dependencies(script, cli, cfg)
            } else {
                source_dependencies(previous.as_ref(), "narration")
            },
        ));
    }
    if dir.join("words.json").is_file() {
        actions.push(action(
            "timings",
            ActionKind::Derive,
            if cli.from.is_none() {
                "fresh_generation"
            } else {
                "native_resume"
            },
            None,
            Some("words.json"),
            if cli.from.is_none() {
                btree([(
                    "narration",
                    hash_json(&spoken_dependencies(script, cli, cfg))?,
                )])
            } else {
                source_dependencies(previous.as_ref(), "timings")
            },
        ));
    }
    for (index, scene) in script.scenes.iter().enumerate() {
        let still_artifact = format!("scene:{}:still", scene.id);
        let still_deps = if cli.from.is_none() {
            btree([("input", image_inputs(scene, script))])
        } else {
            source_dependencies(previous.as_ref(), &still_artifact)
        };
        actions.push(action(
            &still_artifact,
            ActionKind::Generate,
            if cli.from.is_none() {
                "fresh_generation"
            } else {
                "native_resume"
            },
            None,
            Some(&format!("scene-{index:02}.jpg")),
            still_deps,
        ));
        let clip_artifact = format!("scene:{}:video", scene.id);
        let clip_name = format!("scene-{index:02}.mp4");
        actions.push(action(
            &clip_artifact,
            if dir.join(&clip_name).is_file() {
                ActionKind::Generate
            } else {
                ActionKind::Exclude
            },
            if dir.join(&clip_name).is_file() {
                "available"
            } else {
                "not_selected"
            },
            None,
            dir.join(&clip_name).is_file().then_some(clip_name.as_str()),
            if cli.from.is_none() {
                btree([("input", video_inputs(scene, script, cfg))])
            } else {
                source_dependencies(previous.as_ref(), &clip_artifact)
            },
        ));
    }
    for entity in &script.characters {
        for name in [
            format!("character-{}.jpg", crate::images::slug(&entity.id)),
            format!("character-{}-b.jpg", crate::images::slug(&entity.id)),
        ] {
            if dir.join(&name).is_file() {
                let artifact = format!("reference:{name}");
                actions.push(action(
                    &artifact,
                    if cli.from.is_none() {
                        ActionKind::Generate
                    } else {
                        ActionKind::Reuse
                    },
                    if cli.from.is_none() {
                        "fresh_generation"
                    } else {
                        "native_resume"
                    },
                    None,
                    Some(&name),
                    if cli.from.is_none() {
                        btree([
                            ("entity", hash_json(entity)?),
                            ("image_config", hash_json(&config_value(cli, cfg))?),
                        ])
                    } else {
                        source_dependencies(previous.as_ref(), &artifact)
                    },
                ));
            }
        }
    }
    for entity in &script.locations {
        let name = format!("location-{}.jpg", crate::images::slug(&entity.id));
        if dir.join(&name).is_file() {
            let artifact = format!("reference:{name}");
            actions.push(action(
                &artifact,
                if cli.from.is_none() {
                    ActionKind::Generate
                } else {
                    ActionKind::Reuse
                },
                if cli.from.is_none() {
                    "fresh_generation"
                } else {
                    "native_resume"
                },
                None,
                Some(&name),
                if cli.from.is_none() {
                    btree([
                        ("entity", hash_json(entity)?),
                        ("image_config", hash_json(&config_value(cli, cfg))?),
                    ])
                } else {
                    source_dependencies(previous.as_ref(), &artifact)
                },
            ));
        }
    }
    if dir.join("character-ref.jpg").is_file() {
        actions.push(action(
            "reference:character-ref.jpg",
            if cli.from.is_none() {
                ActionKind::Generate
            } else {
                ActionKind::Reuse
            },
            if cli.from.is_none() {
                "fresh_generation"
            } else {
                "native_resume"
            },
            None,
            Some("character-ref.jpg"),
            if cli.from.is_none() {
                btree([(
                    "input",
                    config_value(cli, cfg)["character_ref_sha256"]
                        .as_str()
                        .unwrap_or("generated")
                        .to_owned(),
                )])
            } else {
                source_dependencies(previous.as_ref(), "reference:character-ref.jpg")
            },
        ));
    }
    if let Some(name) = existing_music_name(dir) {
        actions.push(action(
            "music",
            ActionKind::Generate,
            "available",
            None,
            Some(&name),
            if cli.from.is_none() {
                btree([("prompt", hash_bytes(script.music_prompt.as_bytes()))])
            } else {
                source_dependencies(previous.as_ref(), "music")
            },
        ));
    }
    if dir.join("poster.jpg").is_file() {
        actions.push(action(
            "poster",
            if cli.from.is_none() {
                ActionKind::Generate
            } else {
                ActionKind::Reuse
            },
            if cli.from.is_none() {
                "fresh_generation"
            } else {
                "native_resume"
            },
            None,
            Some("poster.jpg"),
            if cli.from.is_none() {
                btree([(
                    "input",
                    hash_json(&json!({
                        "prompt": script.poster_prompt,
                        "character": script.characters.first(),
                        "format": script.format,
                        "image_model": cfg.image_model,
                        "character_ref_sha256": config_value(cli, cfg)["character_ref_sha256"],
                    }))?,
                )])
            } else {
                source_dependencies(previous.as_ref(), "poster")
            },
        ));
    }
    let output_name = if dir.join("reel-video.mp4").is_file() {
        "reel-video.mp4"
    } else {
        "reel.mp4"
    };
    actions.push(action(
        "output",
        ActionKind::Derive,
        "native_render",
        None,
        Some(output_name),
        btree([("config", hash_json(&config_value(cli, cfg))?)]),
    ));
    let artifacts = snapshot(dir)?;
    let manifest = RunManifest {
        version: 1,
        state: "completed".into(),
        run_id: dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("run")
            .into(),
        revision_id: "root".into(),
        parent_revision: None,
        approval_hash: hash_json(
            &json!({"script":script,"config":config_value(cli,cfg),"artifacts":artifacts}),
        )?,
        source: dir.to_path_buf(),
        config: config_value(cli, cfg),
        artifacts,
        actions,
        warnings: Vec::new(),
        outputs: discover_outputs(dir)?,
    };
    atomic_json(&dir.join(MANIFEST), &manifest)
}

#[cfg(test)]
mod tests {
    use super::{copy_atomic, lock_run, pending_accepted_jobs};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp(name: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("reelmaestro-{name}-{nonce}"));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn concurrent_run_lock_is_rejected_and_released_on_drop() {
        let dir = temp("lock");
        let first = lock_run(&dir).unwrap();
        assert!(lock_run(&dir).is_err());
        drop(first);
        assert!(lock_run(&dir).is_ok());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn accepted_provider_job_remains_pending_until_artifact_exists() {
        let dir = temp("pending");
        fs::write(
            dir.join("scene-00.video-job.json"),
            br#"{"job_id":"provider-1","status":"in_progress"}"#,
        )
        .unwrap();
        assert_eq!(pending_accepted_jobs(&dir).unwrap().len(), 1);
        fs::write(dir.join("scene-00.mp4"), b"completed artifact").unwrap();
        assert!(pending_accepted_jobs(&dir).unwrap().is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reused_assets_are_copies_not_hardlinks() {
        let dir = temp("copy");
        let source = dir.join("source");
        let destination = dir.join("destination");
        fs::write(&source, b"original").unwrap();
        copy_atomic(&source, &destination).unwrap();
        fs::write(&destination, b"changed").unwrap();
        assert_eq!(fs::read(source).unwrap(), b"original");
        fs::remove_dir_all(dir).unwrap();
    }
}
