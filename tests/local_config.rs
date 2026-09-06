// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Isolated CLI configuration checks: no credentials, dotenv files, HTTP or GPU access.
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("reelmaestro-config-{nonce}"));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("script.json"), r#"{"title":"fixture","narration":"","music_prompt":"","scenes":[{"line":"","image_prompt":"fox"}]}"#).unwrap();
        for file in ["audio.mp3", "poster.jpg", "scene-00.jpg"] {
            std::fs::write(dir.join(file), []).unwrap();
        }
        Self(dir)
    }

    fn run(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_reelmaestro"));
        cmd.env_clear()
            .current_dir(&self.0)
            .args(["--from", ".", "--dry-run"])
            .args(args);
        for (key, value) in env {
            cmd.env(key, value);
        }
        cmd.output().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn complete_local_resume_needs_no_openrouter_key() {
    let fixture = Fixture::new();
    let out = fixture.run(&["--video-provider", "local"], &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn invalid_provider_env_fails_instead_of_defaulting_to_hosted() {
    let fixture = Fixture::new();
    let out = fixture.run(&[], &[("REELMAESTRO_VIDEO_PROVIDER", "locla")]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid REELMAESTRO_VIDEO_PROVIDER"));
    let overridden = fixture.run(
        &["--video-provider", "local"],
        &[("REELMAESTRO_VIDEO_PROVIDER", "locla")],
    );
    assert!(overridden.status.success());
}

#[test]
fn local_configuration_rejects_incompatible_and_invalid_values() {
    let fixture = Fixture::new();
    for args in [
        vec!["--video-model", "google/veo-3.1"],
        vec!["--video-resolution", "1080p"],
        vec!["--video-seed", "9223372036854775808"],
        vec!["--video-wait-timeout", "0"],
    ] {
        let mut flags = vec!["--video-provider", "local"];
        flags.extend(args);
        assert!(!fixture.run(&flags, &[]).status.success());
    }
    let invalid_steps = fixture.run(
        &["--video-provider", "local"],
        &[("REELMAESTRO_VIDEO_STEPS", "typo")],
    );
    assert!(!invalid_steps.status.success());
}

#[test]
fn missing_poster_cannot_accidentally_start_hosted_generation_without_key() {
    let fixture = Fixture::new();
    std::fs::remove_file(fixture.0.join("poster.jpg")).unwrap();
    let out = fixture.run(&["--video-provider", "local"], &[]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("poster"));
}
