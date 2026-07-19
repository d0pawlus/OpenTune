// SPDX-License-Identifier: GPL-3.0-or-later
//! App-side AI tool executor: maps registry tool calls onto owner-task
//! commands. This is the ONLY path from a model to the app, and it enforces
//! policy + guardrails + audit on every call (ADR-0008). Consumed by the
//! embedded assistant (slice 3) and the MCP server (slice 4).

use std::io::Write;
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

use opentune_ai::{
    registry, validate_change, AuditChannel, AuditOutcome, AuditRecord, CellVerdict, ChangeRequest,
    GuardrailLimits, PermissionPolicy, RateLimiter,
};
use opentune_model::Value;

use crate::dto::{AnomalyThresholdsDto, LogStatsParamsDto, VirtualDynoParamsDto};
use crate::owner::{request, Command, OwnerHandle, Reply};

/// Where audit lines go. The trait keeps tests file-free.
pub trait AuditSink: Send + Sync {
    fn append(&self, line: &str);
}

/// Append-only JSONL file under `app_config_dir` (wired in slice 2 setup).
pub struct FileAuditSink {
    path: std::path::PathBuf,
}

impl FileAuditSink {
    pub fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }
}

impl AuditSink for FileAuditSink {
    fn append(&self, line: &str) {
        // Audit failures must never take the tool path down; a lost audit
        // line is logged to stderr, matching the app's existing style.
        let write = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut f| writeln!(f, "{line}"));
        if let Err(e) = write {
            eprintln!("opentune: audit append failed: {e}");
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolErrorKind {
    Denied,
    InvalidInput,
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolError {
    pub kind: ToolErrorKind,
    pub message: String,
}

impl ToolError {
    fn denied(message: impl Into<String>) -> Self {
        Self {
            kind: ToolErrorKind::Denied,
            message: message.into(),
        }
    }
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            kind: ToolErrorKind::InvalidInput,
            message: message.into(),
        }
    }
    fn failed(message: impl Into<String>) -> Self {
        Self {
            kind: ToolErrorKind::Failed,
            message: message.into(),
        }
    }
}

/// A recorded, validated change proposal awaiting the user (advisory: the
/// user applies it through the normal table-edit path, or ignores it).
/// No `specta::Type` yet — nothing IPC-facing exposes proposals in this
/// slice; slice 3 adds the mirror DTO in `dto.rs` when the UI needs it
/// (deriving it here would force specta onto the pure `opentune-ai` types).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalDto {
    pub id: u32,
    pub constant: String,
    pub reason: String,
    pub ok: bool,
    pub cells: Vec<CellVerdict>,
}

#[derive(Deserialize)]
struct ReadTuneInput {
    names: Vec<String>,
}

#[derive(Deserialize)]
struct RunVeAnalyzeInput {
    table: String,
}

#[derive(Deserialize)]
struct LogToolInput {
    log_id: u32,
    #[serde(default)]
    channels: Vec<String>,
    #[serde(default)]
    thresholds: Option<Json>,
    #[serde(default)]
    params: Option<Json>,
}

#[derive(Deserialize)]
struct ProposedEdit {
    index: u32,
    value: f64,
}

#[derive(Deserialize)]
struct ProposeChangeInput {
    constant: String,
    edits: Vec<ProposedEdit>,
    reason: String,
}

#[derive(Default)]
struct ProposalLog {
    next_id: u32,
    items: Vec<ProposalDto>,
}

/// The executor: policy gate → dispatch → audit, for every call.
pub struct AiToolExecutor {
    owner: OwnerHandle,
    policy: PermissionPolicy,
    limits: GuardrailLimits,
    channel: AuditChannel,
    audit: Box<dyn AuditSink>,
    rate: Mutex<RateLimiter>,
    proposals: Mutex<ProposalLog>,
}

impl AiToolExecutor {
    pub fn new(
        owner: OwnerHandle,
        policy: PermissionPolicy,
        limits: GuardrailLimits,
        channel: AuditChannel,
        audit: Box<dyn AuditSink>,
    ) -> Self {
        Self {
            owner,
            policy,
            limits,
            channel,
            audit,
            rate: Mutex::new(RateLimiter::default()),
            proposals: Mutex::new(ProposalLog::default()),
        }
    }

    /// Proposals recorded so far (slice 3 renders these for the user).
    pub fn proposals(&self) -> Vec<ProposalDto> {
        self.proposals.lock().unwrap().items.clone()
    }

    /// Execute one tool call. Every call — allowed, denied, or failed — is
    /// audited before this returns.
    pub async fn execute(&self, name: &str, input: Json) -> Result<Json, ToolError> {
        let started = Instant::now();
        let result = self.dispatch(name, &input).await;
        let outcome = match &result {
            Ok(_) => AuditOutcome::Ok,
            Err(e) if e.kind == ToolErrorKind::Denied => AuditOutcome::Denied {
                reason: e.message.clone(),
            },
            Err(e) => AuditOutcome::Error {
                message: e.message.clone(),
            },
        };
        let record = AuditRecord {
            t_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            channel: self.channel,
            tool: name.to_owned(),
            input,
            outcome,
            duration_ms: started.elapsed().as_millis() as u64,
        };
        self.audit.append(&record.to_jsonl_line());
        result
    }

    async fn dispatch(&self, name: &str, input: &Json) -> Result<Json, ToolError> {
        let spec = registry()
            .into_iter()
            .find(|t| t.name == name)
            .ok_or_else(|| ToolError::failed(format!("unknown tool: {name}")))?;
        if !self.policy.allows(spec.kind) {
            return Err(ToolError::denied(format!(
                "tool {name} is locked at the current authority level"
            )));
        }
        match name {
            "read_tune" => {
                let args: ReadTuneInput = parse(input)?;
                let values: Vec<Value> = self
                    .owner_request(|reply| Command::GetValues {
                        names: args.names,
                        reply,
                    })
                    .await?;
                Ok(json!({ "values": values }))
            }
            // `last_frame` is never cleared on disconnect (owner.rs) — this
            // can return a snapshot from a link that has since dropped;
            // `age_ms` self-reports staleness and the cutoff is deliberately
            // left to the consumer (slice 3), not enforced here.
            "read_realtime" => {
                let snap = self
                    .owner_request(|reply| Command::RealtimeSnapshot { reply })
                    .await?;
                to_json(&snap)
            }
            "run_ve_analyze" => {
                let args: RunVeAnalyzeInput = parse(input)?;
                let report = self
                    .owner_request(|reply| Command::RunVeAnalyze {
                        table: args.table,
                        reply,
                    })
                    .await?;
                to_json(&report)
            }
            "get_log_stats" => {
                let args: LogToolInput = parse(input)?;
                let params = LogStatsParamsDto {
                    channels: args.channels,
                    reject_when: Vec::new(),
                };
                let report = self
                    .owner_request(|reply| Command::LogStats {
                        log_id: args.log_id,
                        params,
                        reply,
                    })
                    .await?;
                to_json(&report)
            }
            "detect_anomaly" => {
                let args: LogToolInput = parse(input)?;
                let thresholds: AnomalyThresholdsDto = args
                    .thresholds
                    .ok_or_else(|| ToolError::invalid("thresholds required"))
                    .and_then(|v| parse(&v))?;
                let report = self
                    .owner_request(|reply| Command::DetectAnomaly {
                        log_id: args.log_id,
                        thresholds,
                        reply,
                    })
                    .await?;
                to_json(&report)
            }
            "virtual_dyno" => {
                let args: LogToolInput = parse(input)?;
                let params: VirtualDynoParamsDto = args
                    .params
                    .ok_or_else(|| ToolError::invalid("params required"))
                    .and_then(|v| parse(&v))?;
                let report = self
                    .owner_request(|reply| Command::VirtualDyno {
                        log_id: args.log_id,
                        params,
                        reply,
                    })
                    .await?;
                to_json(&report)
            }
            "propose_change" => self.propose_change(input).await,
            // Policy already denied these below `assisted`/`autonomous`;
            // their real implementations arrive with those levels.
            "apply_change" | "burn_now" => Err(ToolError::denied(format!(
                "tool {name} is not implemented in this build"
            ))),
            other => Err(ToolError::failed(format!("unknown tool: {other}"))),
        }
    }

    async fn propose_change(&self, input: &Json) -> Result<Json, ToolError> {
        let args: ProposeChangeInput = parse(input)?;
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        self.rate
            .lock()
            .unwrap()
            .check(now_ms, &self.limits)
            .map_err(|v| ToolError::denied(v.to_string()))?;
        let name = args.constant.clone();
        let bounds = self
            .owner_request(move |reply| Command::ConstantBounds { name, reply })
            .await?;
        let names = vec![args.constant.clone()];
        let current_value: Vec<Value> = self
            .owner_request(|reply| Command::GetValues { names, reply })
            .await?;
        let current: Vec<f64> = match current_value.first() {
            Some(Value::Scalar(v)) => vec![*v],
            Some(Value::Array(vs)) => vs.clone(),
            Some(_) => {
                return Err(ToolError::invalid(
                    "propose_change supports scalar and array constants",
                ))
            }
            None => return Err(ToolError::failed("constant has no current value")),
        };
        let req = ChangeRequest {
            constant: args.constant.clone(),
            edits: args.edits.iter().map(|e| (e.index, e.value)).collect(),
            reason: args.reason.clone(),
        };
        // Advisory never writes, so link health is not enforced here —
        // validate_change keeps the same contract the assisted level will
        // enforce for real. `true` is a placeholder, not a fact: offline
        // sessions (conn: None) load tunes too and reach this path, so the
        // assisted-level implementer must wire a real link-health input.
        let validated = validate_change(&req, bounds, &current, &self.limits, true)
            .map_err(|v| ToolError::denied(v.to_string()))?;
        let proposal = {
            let mut log = self.proposals.lock().unwrap();
            log.next_id += 1;
            let p = ProposalDto {
                id: log.next_id,
                constant: args.constant,
                reason: args.reason,
                ok: validated.ok,
                cells: validated.cells,
            };
            log.items.push(p.clone());
            p
        };
        to_json(&proposal)
    }

    async fn owner_request<T>(
        &self,
        make: impl FnOnce(Reply<T>) -> Command,
    ) -> Result<T, ToolError> {
        request(&self.owner, make).await.map_err(ToolError::failed)
    }
}

fn parse<T: serde::de::DeserializeOwned>(input: &Json) -> Result<T, ToolError> {
    serde_json::from_value(input.clone()).map_err(|e| ToolError::invalid(e.to_string()))
}

fn to_json<T: Serialize>(value: &T) -> Result<Json, ToolError> {
    serde_json::to_value(value).map_err(|e| ToolError::failed(e.to_string()))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::connection::ConnectSource;
    use crate::owner::{spawn_owner_with_emitter, Emitter};
    use opentune_ai::{AuditChannel, GuardrailLimits, PermissionPolicy};

    /// Audit sink that collects lines for assertions.
    #[derive(Default, Clone)]
    struct VecSink(Arc<Mutex<Vec<String>>>);

    impl AuditSink for VecSink {
        fn append(&self, line: &str) {
            self.0.lock().unwrap().push(line.to_owned());
        }
    }

    async fn connected_executor(limits: GuardrailLimits) -> (AiToolExecutor, VecSink) {
        let emit: Emitter = Arc::new(|_| {});
        let owner = spawn_owner_with_emitter(emit);
        request(&owner, |reply| Command::Connect {
            source: ConnectSource::Simulator { ini_path: None },
            reply,
        })
        .await
        .expect("simulator connects");
        request(&owner, |reply| Command::LoadTune { reply })
            .await
            .expect("tune loads");
        let sink = VecSink::default();
        let exec = AiToolExecutor::new(
            owner,
            PermissionPolicy::advisory(),
            limits,
            AuditChannel::Assistant,
            Box::new(sink.clone()),
        );
        (exec, sink)
    }

    #[tokio::test]
    async fn read_tune_returns_named_values() {
        let (exec, _) = connected_executor(GuardrailLimits::default()).await;
        let out = exec
            .execute("read_tune", serde_json::json!({ "names": ["reqFuel"] }))
            .await
            .expect("read_tune succeeds");
        assert!(out["values"].is_array());
        assert_eq!(out["values"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn unknown_tool_fails_and_is_audited() {
        let (exec, sink) = connected_executor(GuardrailLimits::default()).await;
        let err = exec
            .execute("frobnicate", serde_json::json!({}))
            .await
            .unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::Failed);
        let lines = sink.0.lock().unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("frobnicate"));
    }

    #[tokio::test]
    async fn apply_change_is_denied_at_advisory_and_audited() {
        let (exec, sink) = connected_executor(GuardrailLimits::default()).await;
        let err = exec
            .execute("apply_change", serde_json::json!({ "proposal_id": 1 }))
            .await
            .unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::Denied);
        let lines = sink.0.lock().unwrap();
        assert!(
            lines[0].contains("denied"),
            "audit line records the denial: {}",
            lines[0]
        );
    }

    #[tokio::test]
    async fn propose_change_in_range_records_an_ok_proposal_and_writes_nothing() {
        let (exec, _) = connected_executor(GuardrailLimits::default()).await;
        let out = exec
            .execute(
                "propose_change",
                serde_json::json!({
                    "constant": "reqFuel",
                    "edits": [{ "index": 0, "value": 13.0 }],
                    "reason": "smoke"
                }),
            )
            .await
            .expect("proposal recorded");
        assert_eq!(out["ok"], true);
        let proposals = exec.proposals();
        assert_eq!(proposals.len(), 1);
        assert!(proposals[0].ok);
        // No write happened: the tune still reads the original value.
        let read = exec
            .execute("read_tune", serde_json::json!({ "names": ["reqFuel"] }))
            .await
            .expect("read back");
        let read_back = read["values"][0]["Scalar"].as_f64().unwrap_or(f64::NAN);
        assert_ne!(read_back, 13.0, "propose must never write");
    }

    #[tokio::test]
    async fn propose_change_out_of_range_is_flagged_not_written() {
        let (exec, _) = connected_executor(GuardrailLimits::default()).await;
        let out = exec
            .execute(
                "propose_change",
                serde_json::json!({
                    "constant": "reqFuel",
                    "edits": [{ "index": 0, "value": 9999.0 }],
                    "reason": "bad"
                }),
            )
            .await
            .expect("verdicts come back, not an error");
        assert_eq!(out["ok"], false);
    }

    #[tokio::test]
    async fn propose_change_is_rate_limited() {
        let (exec, _) = connected_executor(GuardrailLimits {
            min_interval_ms: 60_000,
            ..GuardrailLimits::default()
        })
        .await;
        let body = serde_json::json!({
            "constant": "reqFuel",
            "edits": [{ "index": 0, "value": 13.0 }],
            "reason": "first"
        });
        exec.execute("propose_change", body.clone())
            .await
            .expect("first passes");
        let err = exec.execute("propose_change", body).await.unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::Denied);
        assert!(err.message.contains("rate limited"));
    }
}
