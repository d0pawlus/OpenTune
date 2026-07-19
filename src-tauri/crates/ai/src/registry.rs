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
            description: "Per-channel summary statistics (min/max/mean/stddev) over an opened datalog, with auditable row-rejection filters.",
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
                        "description": "AnomalyThresholds fields (afrChannel, leanAfr, rpmChannel, knockChannel, ...) exactly as the detect_anomaly IPC command takes them"
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
                        "description": "VirtualDynoParams fields (speedChannel, rpmChannel, massKg, ...) exactly as the virtual_dyno IPC command takes them"
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
