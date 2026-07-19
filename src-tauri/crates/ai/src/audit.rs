// SPDX-License-Identifier: GPL-3.0-or-later
//! Audit records for every AI tool invocation (ADR-0008: audit every AI
//! action — who/what/when/why). The crate defines the record and its JSONL
//! encoding; file I/O belongs to the app layer.

use serde::{Deserialize, Serialize};

/// Which access channel invoked the tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuditChannel {
    Assistant,
    Mcp,
}

/// How the invocation ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AuditOutcome {
    Ok,
    Error { message: String },
    Denied { reason: String },
}

/// One audited tool invocation. `t_unix_ms` is supplied by the caller —
/// this crate never reads the clock.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditRecord {
    pub t_unix_ms: u64,
    pub channel: AuditChannel,
    pub tool: String,
    pub input: serde_json::Value,
    pub outcome: AuditOutcome,
    pub duration_ms: u64,
}

impl AuditRecord {
    /// One JSON object, one line — `serde_json` escapes embedded newlines,
    /// so the single-line property holds for any input.
    pub fn to_jsonl_line(&self) -> String {
        serde_json::to_string(self).expect("audit record serializes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> AuditRecord {
        AuditRecord {
            t_unix_ms: 1_784_454_000_000,
            channel: AuditChannel::Assistant,
            tool: "propose_change".into(),
            input: serde_json::json!({ "constant": "reqFuel", "reason": "line\nbreak" }),
            outcome: AuditOutcome::Denied {
                reason: "advisory level".into(),
            },
            duration_ms: 3,
        }
    }

    #[test]
    fn jsonl_line_is_single_line_and_round_trips() {
        let line = record().to_jsonl_line();
        assert!(!line.contains('\n'), "JSONL lines must not embed newlines");
        let back: AuditRecord = serde_json::from_str(&line).expect("round-trip");
        assert_eq!(back, record());
    }
}
