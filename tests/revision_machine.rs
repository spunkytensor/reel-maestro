// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn temp(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "reelmaestro-revision-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn command(cwd: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_reelmaestro"));
    command.env_clear().current_dir(cwd);
    command
}

fn source(root: &Path) -> PathBuf {
    let source = root.join("run");
    fs::create_dir(&source).unwrap();
    fs::write(
        source.join("script.json"),
        br#"{
          "title":"Offline", "narration":"alpha beta gamma delta",
          "scenes":[
            {"id":"scene-a","line":"alpha beta","image_prompt":"red","cast_ids":[],"location_id":"","transition":"","motion_prompt":"pan"},
            {"id":"scene-b","line":"gamma delta","image_prompt":"blue","cast_ids":[],"location_id":"","transition":"","motion_prompt":"tilt"}
          ],
          "music_prompt":"quiet", "characters":[], "locations":[], "cast":"",
          "poster_prompt":"cover", "narrator_gender":"neutral", "format":"", "chapters":[],
          "description":"", "tags":[]
        }"#,
    )
    .unwrap();
    for (name, bytes) in [
        ("audio.mp3", b"audio".as_slice()),
        ("words.json", br#"[]"#.as_slice()),
        ("scene-00.jpg", b"red-image".as_slice()),
        ("scene-01.jpg", b"blue-image".as_slice()),
        ("poster.jpg", b"poster".as_slice()),
        ("reel.mp4", b"old-output".as_slice()),
    ] {
        fs::write(source.join(name), bytes).unwrap();
    }
    source
}

fn plan(root: &Path, request: serde_json::Value) -> (Output, serde_json::Value) {
    let request_path = root.join("request.json");
    fs::write(&request_path, serde_json::to_vec(&request).unwrap()).unwrap();
    let output = command(root)
        .args(["--no-dotenv", "--revision-plan"])
        .arg(&request_path)
        .output()
        .unwrap();
    let value = serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null);
    (output, value)
}

fn write_color_jpeg(path: &Path, color: &str) {
    let status = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("color=c={color}:s=360x640"))
        .args(["-frames:v", "1"])
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn planning_is_offline_side_effect_free_and_dependency_aware() {
    let root = temp("plan");
    let source = source(&root);
    let before: Vec<_> = fs::read_dir(&source)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    let request = serde_json::json!({
        "version":1, "source":source, "revision_id":"rev-1", "legacy_reuse":"trust",
        "arguments":["--music-action","remove"], "job_id":"job-1"
    });
    let (output, value) = plan(&root, request);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(value["version"], 1);
    assert!(value["approval_hash"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert_eq!(value["cost"]["total_usd"], 0.0);
    assert_eq!(value["request"]["script"]["scenes"][0]["id"], "scene-a");
    let after: Vec<_> = fs::read_dir(&source)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(before, after, "planning changed its source folder");

    let mut edited = value["request"]["script"].clone();
    edited["scenes"].as_array_mut().unwrap().swap(0, 1);
    edited["narration"] = serde_json::json!("gamma delta alpha beta");
    let request = serde_json::json!({
        "version":1, "source":source, "revision_id":"rev-2", "legacy_reuse":"trust",
        "script":edited, "arguments":["--music-action","remove"]
    });
    let (output, changed) = plan(&root, request);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actions = changed["actions"].as_array().unwrap();
    let first = actions
        .iter()
        .find(|a| a["artifact"] == "scene:scene-b:still")
        .unwrap();
    assert_eq!(first["action"], "reuse");
    assert_eq!(first["source"], "scene-01.jpg");
    assert_eq!(first["destination"], "scene-00.jpg");
    let narration = actions
        .iter()
        .find(|a| a["artifact"] == "narration")
        .unwrap();
    assert_eq!(narration["action"], "generate");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn execute_rejects_stale_source_before_creating_a_draft() {
    let root = temp("stale");
    let source = source(&root);
    let request = serde_json::json!({
        "version":1, "source":source, "revision_id":"stale-rev", "legacy_reuse":"trust",
        "arguments":["--music-action","remove"], "job_id":"job-stale"
    });
    let (output, value) = plan(&root, request);
    assert!(output.status.success());
    let plan_path = root.join("plan.json");
    fs::write(&plan_path, serde_json::to_vec(&value).unwrap()).unwrap();
    fs::write(source.join("scene-00.jpg"), b"host edit").unwrap();
    let output = command(&root)
        .args(["--no-dotenv", "--revision-execute"])
        .arg(&plan_path)
        .args([
            "--approval-hash",
            value["approval_hash"].as_str().unwrap(),
            "--events-json",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let events: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events.last().unwrap()["state"], "failed");
    assert_eq!(events.last().unwrap()["code"], "stale_plan");
    assert!(
        !source.join("revisions").exists(),
        "stale execution wrote a draft"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn execute_rejects_tampered_plan_even_with_the_original_approval_hash() {
    let root = temp("tampered");
    let source = source(&root);
    let request = serde_json::json!({
        "version":1, "source":source, "revision_id":"tampered-rev", "legacy_reuse":"trust",
        "arguments":["--music-action","remove"]
    });
    let (output, mut value) = plan(&root, request);
    assert!(output.status.success());
    let approval = value["approval_hash"].as_str().unwrap().to_owned();
    value["destination"] = serde_json::json!(root.join("attacker-selected"));
    let plan_path = root.join("tampered-plan.json");
    fs::write(&plan_path, serde_json::to_vec(&value).unwrap()).unwrap();
    let output = command(&root)
        .args(["--no-dotenv", "--revision-execute"])
        .arg(&plan_path)
        .args(["--approval-hash", &approval, "--events-json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let first: serde_json::Value = serde_json::from_str(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(first["code"], "plan_tampered");
    assert!(!root.join("attacker-selected").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn recover_refuses_ambiguous_paid_work_without_resubmitting() {
    let root = temp("ambiguous");
    let source = source(&root);
    let request = serde_json::json!({
        "version":1, "source":source, "revision_id":"ambiguous-rev", "legacy_reuse":"trust",
        "arguments":["--regenerate-image-scene-id","scene-a","--music-action","remove"]
    });
    let (output, value) = plan(&root, request);
    assert!(output.status.success());
    let plan_path = root.join("ambiguous-plan.json");
    fs::write(&plan_path, serde_json::to_vec(&value).unwrap()).unwrap();
    let work = source.join("revisions/.ambiguous-rev.work");
    fs::create_dir_all(&work).unwrap();
    fs::write(
        work.join(".revision-execution.json"),
        serde_json::to_vec(&serde_json::json!({
            "version":1,
            "approval_hash":value["approval_hash"],
            "state":"running"
        }))
        .unwrap(),
    )
    .unwrap();
    let output = command(&root)
        .args(["--no-dotenv", "--revision-recover"])
        .arg(&plan_path)
        .args([
            "--approval-hash",
            value["approval_hash"].as_str().unwrap(),
            "--events-json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(75));
    let events: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events.last().unwrap()["code"], "ambiguous_paid_work");
    assert!(!work.join("scene-00.jpg").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_revision_reorders_copied_stills_removes_music_and_publishes_valid_video() {
    let root = temp("execute-local");
    let source = source(&root);
    write_color_jpeg(&source.join("scene-00.jpg"), "red");
    write_color_jpeg(&source.join("scene-01.jpg"), "blue");
    write_color_jpeg(&source.join("poster.jpg"), "green");
    fs::write(source.join("music.wav"), b"must not be copied").unwrap();
    let red_hash = sha256(&source.join("scene-00.jpg"));
    let blue_hash = sha256(&source.join("scene-01.jpg"));
    let mut edited: serde_json::Value =
        serde_json::from_slice(&fs::read(source.join("script.json")).unwrap()).unwrap();
    edited["scenes"].as_array_mut().unwrap().swap(0, 1);
    edited["narration"] = serde_json::json!("gamma delta alpha beta");
    let request = serde_json::json!({
        "version":1, "source":source, "revision_id":"local-reorder", "legacy_reuse":"trust",
        "script":edited,
        "arguments":["--no-narration","--scene-seconds","1","--no-captions",
          "--music-action","remove","--no-grade","--no-loudnorm","--no-dissolve",
          "--no-embed-poster","--export-preset","web"],
        "job_id":"offline-zero-provider-calls"
    });
    let (output, value) = plan(&root, request);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(value["cost"]["total_usd"], 0.0);
    let plan_path = root.join("local-plan.json");
    fs::write(&plan_path, serde_json::to_vec(&value).unwrap()).unwrap();
    let output = command(&root)
        .args(["--no-dotenv", "--revision-execute"])
        .arg(&plan_path)
        .args([
            "--approval-hash",
            value["approval_hash"].as_str().unwrap(),
            "--events-json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let completed = events.last().unwrap();
    assert_eq!(completed["stage"], "publish");
    assert_eq!(completed["state"], "completed");
    let destination = root.join("run/revisions/local-reorder");
    assert_eq!(sha256(&destination.join("scene-00.jpg")), blue_hash);
    assert_eq!(sha256(&destination.join("scene-01.jpg")), red_hash);
    assert!(!destination.join("music.wav").exists());
    assert!(destination.join("reel.mp4").is_file());
    assert_eq!(
        Path::new(completed["artifact"].as_str().unwrap()),
        destination.join("reel.mp4")
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(destination.join("run-manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["state"], "completed");
    assert!(manifest["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|artifact| {
            artifact["path"] == "reel.mp4"
                && artifact["sha256"].as_str().unwrap().starts_with("sha256:")
        }));
    let request = serde_json::json!({
        "version":1, "source":destination, "revision_id":"inherited-controls",
        "legacy_reuse":"reject", "arguments":["--music-action","remove"]
    });
    let (output, inherited) = plan(&root, request);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(inherited["resolved_config"]["no_narration"], true);
    assert_eq!(inherited["resolved_config"]["no_captions"], true);
    assert_eq!(inherited["resolved_config"]["scene_seconds"], 1.0);
    assert_eq!(inherited["resolved_config"]["export_preset"], "web");
    let inherited_still = inherited["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["artifact"] == "scene:scene-b:still")
        .unwrap();
    let original_still = manifest["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["artifact"] == "scene:scene-b:still")
        .unwrap();
    assert_eq!(inherited_still["action"], "reuse");
    assert_eq!(
        inherited_still["dependencies"],
        original_still["dependencies"]
    );
    fs::remove_dir_all(root).unwrap();
}

fn sha256(path: &Path) -> String {
    use std::io::Read;
    let mut file = fs::File::open(path).unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    use sha2::{Digest, Sha256};
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[test]
fn explicit_scene_actions_are_validated_without_provider_calls() {
    let root = temp("actions");
    let source = source(&root);
    let request = serde_json::json!({
        "version":1, "source":source, "revision_id":"actions", "legacy_reuse":"trust",
        "arguments":["--regenerate-image-scene-id","scene-a","--video-scene-id","scene-b","--regenerate-video-scene-id","scene-b","--music-action","remove"]
    });
    let (output, value) = plan(&root, request);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actions = value["actions"].as_array().unwrap();
    assert_eq!(
        actions
            .iter()
            .find(|a| a["artifact"] == "scene:scene-a:still")
            .unwrap()["reason"],
        "explicit_regenerate"
    );
    assert_eq!(
        actions
            .iter()
            .find(|a| a["artifact"] == "scene:scene-b:video")
            .unwrap()["action"],
        "generate"
    );
    assert!(value["cost"]["images_usd"].as_f64().unwrap() > 0.0);
    assert!(value["cost"]["video_usd"].as_f64().unwrap() > 0.0);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn inspect_run_assigns_canonical_legacy_ids_and_reports_hashed_outputs() {
    let root = temp("inspect");
    let source = source(&root);
    let mut script: serde_json::Value =
        serde_json::from_slice(&fs::read(source.join("script.json")).unwrap()).unwrap();
    for scene in script["scenes"].as_array_mut().unwrap() {
        scene.as_object_mut().unwrap().remove("id");
    }
    fs::write(
        source.join("script.json"),
        serde_json::to_vec(&script).unwrap(),
    )
    .unwrap();
    let output = command(&root)
        .args(["--no-dotenv", "--studio-inspect-run"])
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let inspected: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(inspected["version"], 1);
    assert_eq!(inspected["manifest_state"], "legacy");
    assert!(inspected["source_fingerprint"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert!(inspected["script"]["scenes"][0]["id"]
        .as_str()
        .unwrap()
        .starts_with("scene-"));
    assert!(inspected["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|output| output["path"] == "reel.mp4" && output["sha256"].is_string()));
    fs::remove_dir_all(root).unwrap();
}
