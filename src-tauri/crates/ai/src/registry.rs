// SPDX-License-Identifier: GPL-3.0-or-later
//! The shared tool registry: one definition feeding both the embedded
//! assistant (slice 3) and the MCP server (slice 4). Schemas are
//! hand-written `serde_json` values (ponytail: 9 small objects — a schema
//! derive dependency would outweigh them).

use serde_json::{json, Value};

use crate::{PermissionPolicy, ToolKind};

/// One tool as advertised to a model: name, prose contract, policy kind,
/// and a JSON-schema object describing the input.
#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub kind: ToolKind,
    pub input_schema: Value,
}

/// The full registry in stable order. Deterministic: same call, same list.
pub fn registry() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "read_tune",
            description: "Read current values of named tune constants (scalars, enums, or table arrays).",
            kind: ToolKind::ReadOnly,
            input_schema: json!({
                "type": "object",
                "properties": {
                    "names": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "INI constant names, e.g. [\"reqFuel\", \"veTable1Tbl\"]"
                    }
                },
                "required": ["names"]
            }),
        },
        ToolSpec {
            name: "read_realtime",
            description: "Latest realtime channel snapshot from the live connection (empty until realtime polling has produced a frame).",
            kind: ToolKind::ReadOnly,
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        ToolSpec {
            name: "run_ve_analyze",
            description: "Run the deterministic VE-correction engine against the current realtime capture for a named [TableEditor] table. Returns per-cell proposals with confidence and filter counts.",
            kind: ToolKind::ReadOnly,
            input_schema: json!({
                "type": "object",
                "properties": {
                    "table": { "type": "string", "description": "Table id, e.g. \"veTable1Tbl\"" }
                },
                "required": ["table"]
            }),
        },
        ToolSpec {
            name: "get_log_stats",
            description: "Per-channel summary statistics (min/max/mean/stddev) over an opened datalog.",
            kind: ToolKind::ReadOnly,
            input_schema: json!({
                "type": "object",
                "properties": {
                    "log_id": { "type": "integer", "description": "Id returned when the log was opened" },
                    "channels": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Channels to summarize; empty or omitted = all"
                    }
                },
                "required": ["log_id"]
            }),
        },
        // Field names mirror the DTOs in src-tauri/src/dto.rs (snake_case)
        // — the DTOs are the source of truth.
        ToolSpec {
            name: "detect_anomaly",
            description: "Scan an opened datalog for sensor dropouts, lean spikes, and knock using explicit thresholds. Every finding carries row, time, channel, and the rule that fired.",
            kind: ToolKind::ReadOnly,
            input_schema: json!({
                "type": "object",
                "properties": {
                    "log_id": { "type": "integer" },
                    "thresholds": {
                        "type": "object",
                        "description": "AnomalyThresholdsDto fields exactly as the detect_anomaly IPC command takes them",
                        "properties": {
                            "sensors": {
                                "type": "array",
                                "description": "Per-channel [min, max] dropout bounds",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "channel": { "type": "string" },
                                        "min": { "type": "number" },
                                        "max": { "type": "number" }
                                    },
                                    "required": ["channel", "min", "max"]
                                }
                            },
                            "afr_channel": { "type": "string" },
                            "lean_afr": { "type": "number" },
                            "lean_min_rpm": { "type": "number" },
                            "rpm_channel": { "type": "string" },
                            "load_channel": { "type": "string" },
                            "lean_min_load": { "type": "number" },
                            "knock_channel": { "type": "string" },
                            "knock_threshold": { "type": "number" },
                            "knock_min_rpm": { "type": "number" }
                        },
                        "required": [
                            "sensors",
                            "afr_channel",
                            "lean_afr",
                            "lean_min_rpm",
                            "rpm_channel",
                            "load_channel",
                            "lean_min_load",
                            "knock_channel",
                            "knock_threshold",
                            "knock_min_rpm"
                        ]
                    }
                },
                "required": ["log_id", "thresholds"]
            }),
        },
        ToolSpec {
            name: "virtual_dyno",
            description: "Estimate wheel/engine power and torque from an opened datalog using explicit vehicle parameters. The result lists every physical assumption made.",
            kind: ToolKind::ReadOnly,
            input_schema: json!({
                "type": "object",
                "properties": {
                    "log_id": { "type": "integer" },
                    "params": {
                        "type": "object",
                        "description": "VirtualDynoParamsDto fields exactly as the virtual_dyno IPC command takes them",
                        "properties": {
                            "speed_channel": { "type": "string" },
                            "rpm_channel": { "type": "string" },
                            "mass_kg": { "type": "number" },
                            "drag_coefficient": { "type": "number" },
                            "frontal_area_m2": { "type": "number" },
                            "rolling_resistance": { "type": "number" },
                            "drivetrain_loss": { "type": "number" },
                            "smoothing_window": { "type": "integer" },
                            "air_density_kg_m3": { "type": "number" }
                        },
                        "required": [
                            "speed_channel",
                            "rpm_channel",
                            "mass_kg",
                            "drag_coefficient",
                            "frontal_area_m2",
                            "rolling_resistance",
                            "drivetrain_loss",
                            "smoothing_window",
                            "air_density_kg_m3"
                        ]
                    }
                },
                "required": ["log_id", "params"]
            }),
        },
        ToolSpec {
            name: "propose_change",
            description: "Propose (never apply) a change to a tune constant. The proposal is validated against INI low/high bounds and guardrail limits, recorded for the user, and returned with a per-cell verdict.",
            kind: ToolKind::ProposeChange,
            input_schema: json!({
                "type": "object",
                "properties": {
                    "constant": { "type": "string", "description": "INI constant name" },
                    "edits": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "index": { "type": "integer", "description": "Flat cell index; 0 for scalars" },
                                "value": { "type": "number" }
                            },
                            "required": ["index", "value"]
                        }
                    },
                    "reason": { "type": "string", "description": "Why — shown to the user verbatim" }
                },
                "required": ["constant", "edits", "reason"]
            }),
        },
        ToolSpec {
            name: "apply_change",
            description: "Apply a previously validated proposal to the ECU. Locked below the assisted authority level.",
            kind: ToolKind::ApplyChange,
            input_schema: json!({
                "type": "object",
                "properties": {
                    "proposal_id": { "type": "integer" }
                },
                "required": ["proposal_id"]
            }),
        },
        ToolSpec {
            name: "burn_now",
            description: "Burn dirty pages to ECU flash. Locked below the autonomous authority level.",
            kind: ToolKind::Burn,
            input_schema: json!({ "type": "object", "properties": {} }),
        },
    ]
}

/// The registry filtered to what `policy` permits — what a model is shown.
pub fn available_tools(policy: &PermissionPolicy) -> Vec<ToolSpec> {
    registry()
        .into_iter()
        .filter(|t| policy.allows(t.kind))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_nine_uniquely_named_tools_in_stable_order() {
        let tools = registry();
        let names: Vec<&str> = tools.iter().map(|t| t.name).collect();
        assert_eq!(
            names,
            vec![
                "read_tune",
                "read_realtime",
                "run_ve_analyze",
                "get_log_stats",
                "detect_anomaly",
                "virtual_dyno",
                "propose_change",
                "apply_change",
                "burn_now",
            ]
        );
    }

    #[test]
    fn every_schema_is_an_object_schema() {
        for tool in registry() {
            assert_eq!(
                tool.input_schema["type"], "object",
                "{} schema must be an object schema",
                tool.name
            );
            assert!(
                tool.input_schema["properties"].is_object(),
                "{} schema must declare properties",
                tool.name
            );
            assert!(
                !tool.description.is_empty(),
                "{} needs a description",
                tool.name
            );
        }
    }

    #[test]
    fn advisory_filter_excludes_apply_and_burn() {
        let allowed = available_tools(&PermissionPolicy::advisory());
        let names: Vec<&str> = allowed.iter().map(|t| t.name).collect();
        assert!(names.contains(&"read_tune"));
        assert!(names.contains(&"propose_change"));
        assert!(!names.contains(&"apply_change"));
        assert!(!names.contains(&"burn_now"));
    }
}
