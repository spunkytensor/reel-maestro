// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeSet, fs, path::PathBuf, process::Command};

fn isolated_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "reelmaestro-{name}-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn reelmaestro(dir: &PathBuf) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_reelmaestro"));
    command.env_clear().env("PATH", "").current_dir(dir);
    command
}

#[test]
fn schema_is_keyless_and_covers_every_ordinary_flag() {
    let dir = isolated_dir("schema");
    let output = reelmaestro(&dir).arg("--studio-schema").output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let schema: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(schema["version"], 1);
    assert_eq!(schema["availability"]["editing"], true);
    assert_eq!(schema["availability"]["revision_execution"], true);

    let schema_flags: BTreeSet<String> = schema["arguments"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|arg| arg["long"].as_str().map(str::to_owned))
        .collect();
    let help = reelmaestro(&dir).arg("--help").output().unwrap();
    let help = String::from_utf8(help.stdout).unwrap();
    let help_flags: BTreeSet<String> = help
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("--"))
        .filter_map(|line| line.split_whitespace().next())
        .map(|word| format!("--{}", word.trim_end_matches([',', '>'])))
        .filter(|flag| {
            !matches!(
                flag.as_str(),
                "--help"
                    | "--version"
                    | "--studio-schema"
                    | "--studio-estimate"
                    | "--studio-inspect-run"
                    | "--revision-plan"
                    | "--revision-execute"
                    | "--revision-recover"
                    | "--approval-hash"
                    | "--events-json"
            )
        })
        .collect();
    assert_eq!(schema_flags, help_flags);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn estimate_is_keyless_side_effect_free_and_rejects_resume() {
    let dir = isolated_dir("estimate");
    let out = dir.join("must-not-exist");
    let output = reelmaestro(&dir)
        .args([
            "--studio-estimate",
            "--url",
            "http://127.0.0.1:1/nope",
            "--out",
        ])
        .arg(&out)
        .args(["--quality", "draft", "--music-gen"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!out.exists());
    let estimate: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(estimate["version"], 1);
    assert!(estimate["total_usd"].as_f64().unwrap() > 0.0);
    for field in [
        "script_usd",
        "narration_usd",
        "images_usd",
        "video_usd",
        "music_usd",
    ] {
        assert!(estimate[field].is_number(), "missing {field}");
    }
    assert!(estimate["warning"].is_string());

    let rejected = reelmaestro(&dir)
        .args(["--studio-estimate", "--from", "previous"])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("only supports fresh generation"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn ordinary_execution_still_requires_a_key() {
    let dir = isolated_dir("ordinary");
    let output = reelmaestro(&dir)
        .arg("--topic")
        .arg("test")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("OPENROUTER_API_KEY is not set"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn studio_can_disable_ancestor_dotenv_without_changing_native_loading() {
    let dir = isolated_dir("dotenv");
    let nested = dir.join("out").join("job");
    fs::create_dir_all(&nested).unwrap();
    let clean = reelmaestro(&nested)
        .args(["--studio-estimate", "--topic", "test"])
        .output()
        .unwrap();
    assert!(clean.status.success());
    fs::write(dir.join(".env"), "REELMAESTRO_QUALITY=premium\n").unwrap();
    let native = reelmaestro(&nested)
        .args(["--studio-estimate", "--topic", "test"])
        .output()
        .unwrap();
    assert!(native.status.success());
    assert_ne!(
        native.stdout, clean.stdout,
        "native CLI still loads parent .env"
    );
    let isolated = reelmaestro(&nested)
        .args(["--no-dotenv", "--studio-estimate", "--topic", "test"])
        .output()
        .unwrap();
    assert!(isolated.status.success());
    assert_eq!(
        isolated.stdout, clean.stdout,
        "Studio must ignore ancestor .env"
    );
    fs::remove_dir_all(dir).unwrap();
}
