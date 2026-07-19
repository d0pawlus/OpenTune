// SPDX-License-Identifier: GPL-3.0-or-later
//! AI tool layer contracts: permission policy, tool registry, guardrails,
//! audit records. Pure data + logic — no network, no I/O, no clock
//! (timestamps are always inputs). App glue lives in the main crate
//! (`src/ai_tools.rs`), mirroring how `opentune-analysis` is bridged.

/// How much authority the AI has. Ships hardwired to `Advisory`;
/// `Assisted`/`Autonomous` are designed for but deliberately not reachable
/// from any UI (ADR-0008: authority is configuration, not code).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityLevel {
    Advisory,
    Assisted,
    Autonomous,
}

/// What a tool does to the ECU, from the policy's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    ReadOnly,
    ProposeChange,
    ApplyChange,
    Burn,
}

/// The permission policy: a pure predicate from (level, tool kind) to
/// allowed. The engine is identical at every level — only this gate moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionPolicy {
    pub level: AuthorityLevel,
}

impl PermissionPolicy {
    /// The shipping default: read + propose, never write.
    pub const fn advisory() -> Self {
        Self {
            level: AuthorityLevel::Advisory,
        }
    }

    pub fn allows(&self, kind: ToolKind) -> bool {
        match kind {
            ToolKind::ReadOnly | ToolKind::ProposeChange => true,
            ToolKind::ApplyChange => {
                matches!(
                    self.level,
                    AuthorityLevel::Assisted | AuthorityLevel::Autonomous
                )
            }
            ToolKind::Burn => matches!(self.level, AuthorityLevel::Autonomous),
        }
    }
}

mod registry;
pub use registry::{available_tools, registry, ToolSpec};

mod guardrails;
pub use guardrails::{
    validate_change, CellCheck, CellVerdict, ChangeRequest, GuardrailLimits, GuardrailViolation,
    RateLimiter, ValidatedChange,
};

mod audit;
pub use audit::{AuditChannel, AuditOutcome, AuditRecord};

#[cfg(test)]
mod policy_tests {
    use super::*;

    #[test]
    fn advisory_allows_reads_and_proposals_only() {
        let p = PermissionPolicy::advisory();
        assert!(p.allows(ToolKind::ReadOnly));
        assert!(p.allows(ToolKind::ProposeChange));
        assert!(!p.allows(ToolKind::ApplyChange));
        assert!(!p.allows(ToolKind::Burn));
    }

    #[test]
    fn assisted_unlocks_apply_but_not_burn() {
        let p = PermissionPolicy {
            level: AuthorityLevel::Assisted,
        };
        assert!(p.allows(ToolKind::ReadOnly));
        assert!(p.allows(ToolKind::ProposeChange));
        assert!(p.allows(ToolKind::ApplyChange));
        assert!(!p.allows(ToolKind::Burn));
    }

    #[test]
    fn autonomous_unlocks_everything() {
        let p = PermissionPolicy {
            level: AuthorityLevel::Autonomous,
        };
        assert!(p.allows(ToolKind::ApplyChange));
        assert!(p.allows(ToolKind::Burn));
    }
}
