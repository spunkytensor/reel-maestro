// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Stable, side-effect-free machine-readable interfaces used by Reel Maestro Studio.

use anyhow::Result;
use clap::{ArgAction, Command};
use serde_json::{json, Value};

use crate::config::CostEstimate;

const MACHINE_FLAGS: [&str; 8] = [
    "studio_inspect_run",
    "studio_schema",
    "studio_estimate",
    "revision_plan",
    "revision_execute",
    "revision_recover",
    "approval_hash",
    "events_json",
];

/// Emit the UI-facing CLI schema. Machine protocol switches are deliberately kept separate from
/// ordinary arguments so consumers cannot accidentally present them as generation controls.
pub fn print_schema(command: Command) -> Result<()> {
    let arguments: Vec<Value> = command
        .get_arguments()
        .filter(|arg| !MACHINE_FLAGS.contains(&arg.get_id().as_str()))
        .map(|arg| {
            let choices: Vec<String> = arg
                .get_value_parser()
                .possible_values()
                .map(|values| values.map(|v| v.get_name().to_owned()).collect())
                .unwrap_or_default();
            let defaults: Vec<String> = arg
                .get_default_values()
                .iter()
                .map(|value| value.to_string_lossy().into_owned())
                .collect();
            json!({
                "name": arg.get_id().as_str(),
                "long": arg.get_long().map(|long| format!("--{long}")),
                "help": arg.get_help().map(|help| help.to_string()),
                "required": arg.is_required_set(),
                "takes_value": !matches!(arg.get_action(), ArgAction::SetTrue | ArgAction::SetFalse),
                "default_values": defaults,
                "choices": choices,
            })
        })
        .collect();

    let document = json!({
        "version": 1,
        "command": command.get_name(),
        "arguments": arguments,
        "machine_flags": ["--studio-schema", "--studio-estimate", "--studio-inspect-run", "--revision-plan", "--revision-execute", "--revision-recover", "--approval-hash", "--events-json"],
        "availability": {
            "fresh_generation": true,
            "resume": true,
            "editing": true,
            "revision_execution": true,
            "revision_protocol": 1,
            "event_protocol": 1
        }
    });
    println!("{}", serde_json::to_string(&document)?);
    Ok(())
}

/// Emit the exact version-1 cost contract consumed by Studio.
pub fn print_estimate(estimate: &CostEstimate) -> Result<()> {
    let document = json!({
        "version": 1,
        "total_usd": estimate.total(),
        "script_usd": estimate.script,
        "narration_usd": estimate.narration,
        "images_usd": estimate.images,
        "video_usd": estimate.video,
        "music_usd": estimate.music,
        "warning": "Planning estimate only; actual provider billing may differ."
    });
    println!("{}", serde_json::to_string(&document)?);
    Ok(())
}
