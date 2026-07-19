// SPDX-License-Identifier: GPL-3.0-or-later
//! Guardrails for mutating AI actions (ADR-0008: guardrails live in the
//! tool layer, not the prompt). Pure: bounds and current values are inputs,
//! the clock is an input, nothing here talks to the wire.

use serde::Serialize;

/// A change the AI wants made: flat cell edits on one constant (index 0 for
/// scalars), plus its stated reason (audited and shown verbatim).
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeRequest {
    pub constant: String,
    pub edits: Vec<(u32, f64)>,
    pub reason: String,
}

/// Hard limits on a single mutating call. Defaults per M7 decision D-7.
#[derive(Debug, Clone, PartialEq)]
pub struct GuardrailLimits {
    /// Max |delta| as % of the current cell value.
    pub max_delta_pct: f64,
    /// Max cells one call may touch.
    pub max_cells_per_change: usize,
    /// Min spacing between mutating calls.
    pub min_interval_ms: u64,
}

impl Default for GuardrailLimits {
    fn default() -> Self {
        Self {
            max_delta_pct: 15.0,
            max_cells_per_change: 64,
            min_interval_ms: 1000,
        }
    }
}

/// Request-level rejection: nothing about the request is salvageable.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GuardrailViolation {
    LinkUnhealthy,
    TooManyCells { count: usize, max: usize },
    RateLimited { wait_ms: u64 },
}

impl std::fmt::Display for GuardrailViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LinkUnhealthy => write!(f, "connection is not healthy"),
            Self::TooManyCells { count, max } => {
                write!(f, "{count} cells exceeds the per-change limit of {max}")
            }
            Self::RateLimited { wait_ms } => {
                write!(f, "rate limited; retry in {wait_ms} ms")
            }
        }
    }
}

/// Per-cell verdict: bounds first, then delta magnitude.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CellCheck {
    Ok,
    UnknownIndex { len: usize },
    OutOfRange { low: f64, high: f64 },
    DeltaTooLarge { delta_pct: f64, max: f64 },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellVerdict {
    pub index: u32,
    pub value: f64,
    pub check: CellCheck,
}

/// The validated form of a [`ChangeRequest`]: every cell judged, `ok` only
/// when every verdict is [`CellCheck::Ok`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedChange {
    pub cells: Vec<CellVerdict>,
    pub ok: bool,
}

/// Judge a change against INI bounds, current values, and limits.
/// Request-level problems reject wholesale; cell-level problems come back
/// as verdicts so the user sees exactly which cell failed and why.
pub fn validate_change(
    req: &ChangeRequest,
    bounds: (f64, f64),
    current: &[f64],
    limits: &GuardrailLimits,
    link_healthy: bool,
) -> Result<ValidatedChange, GuardrailViolation> {
    if !link_healthy {
        return Err(GuardrailViolation::LinkUnhealthy);
    }
    if req.edits.len() > limits.max_cells_per_change {
        return Err(GuardrailViolation::TooManyCells {
            count: req.edits.len(),
            max: limits.max_cells_per_change,
        });
    }
    let (low, high) = (bounds.0.min(bounds.1), bounds.0.max(bounds.1));
    let cells: Vec<CellVerdict> = req
        .edits
        .iter()
        .map(|&(index, value)| {
            let check = match current.get(index as usize) {
                None => CellCheck::UnknownIndex { len: current.len() },
                Some(_) if !(low..=high).contains(&value) => CellCheck::OutOfRange { low, high },
                Some(&cur) if cur != 0.0 => {
                    let delta_pct = ((value - cur) / cur).abs() * 100.0;
                    if delta_pct > limits.max_delta_pct {
                        CellCheck::DeltaTooLarge {
                            delta_pct,
                            max: limits.max_delta_pct,
                        }
                    } else {
                        CellCheck::Ok
                    }
                }
                // ponytail: no delta guard when current == 0 (undefined %);
                // the bounds check above still applies. Revisit with an
                // absolute-delta limit if zero-cells become common.
                Some(_) => CellCheck::Ok,
            };
            CellVerdict {
                index,
                value,
                check,
            }
        })
        .collect();
    let ok = cells.iter().all(|c| c.check == CellCheck::Ok);
    Ok(ValidatedChange { cells, ok })
}

/// Minimum spacing between mutating calls. Pure: the caller supplies the
/// clock, so tests are deterministic.
#[derive(Debug, Default)]
pub struct RateLimiter {
    last_ms: Option<u64>,
}

impl RateLimiter {
    pub fn check(
        &mut self,
        now_ms: u64,
        limits: &GuardrailLimits,
    ) -> Result<(), GuardrailViolation> {
        if let Some(last) = self.last_ms {
            let elapsed = now_ms.saturating_sub(last);
            if elapsed < limits.min_interval_ms {
                return Err(GuardrailViolation::RateLimited {
                    wait_ms: limits.min_interval_ms - elapsed,
                });
            }
        }
        self.last_ms = Some(now_ms);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(edits: Vec<(u32, f64)>) -> ChangeRequest {
        ChangeRequest {
            constant: "reqFuel".into(),
            edits,
            reason: "test".into(),
        }
    }

    #[test]
    fn in_range_small_delta_passes() {
        let v = validate_change(
            &req(vec![(0, 13.0)]),
            (0.0, 25.5),
            &[12.5],
            &GuardrailLimits::default(),
            true,
        )
        .expect("no request-level violation");
        assert!(v.ok);
        assert_eq!(v.cells[0].check, CellCheck::Ok);
    }

    #[test]
    fn unhealthy_link_rejects_the_whole_request() {
        let err = validate_change(
            &req(vec![(0, 13.0)]),
            (0.0, 25.5),
            &[12.5],
            &GuardrailLimits::default(),
            false,
        )
        .unwrap_err();
        assert_eq!(err, GuardrailViolation::LinkUnhealthy);
    }

    #[test]
    fn out_of_bounds_value_is_flagged_per_cell() {
        let v = validate_change(
            &req(vec![(0, 9999.0)]),
            (0.0, 25.5),
            &[12.5],
            &GuardrailLimits::default(),
            true,
        )
        .expect("cell-level problems are verdicts, not request errors");
        assert!(!v.ok);
        assert_eq!(
            v.cells[0].check,
            CellCheck::OutOfRange {
                low: 0.0,
                high: 25.5
            }
        );
    }

    #[test]
    fn oversized_delta_is_flagged_per_cell() {
        let limits = GuardrailLimits {
            max_delta_pct: 1.0,
            ..GuardrailLimits::default()
        };
        let v = validate_change(&req(vec![(0, 13.125)]), (0.0, 25.5), &[12.5], &limits, true)
            .expect("verdicts");
        assert!(!v.ok);
        match v.cells[0].check {
            CellCheck::DeltaTooLarge { delta_pct, max } => {
                assert!((delta_pct - 5.0).abs() < 1e-9);
                assert!((max - 1.0).abs() < 1e-9);
            }
            ref other => panic!("expected DeltaTooLarge, got {other:?}"),
        }
    }

    #[test]
    fn unknown_index_is_flagged_per_cell() {
        let v = validate_change(
            &req(vec![(7, 13.0)]),
            (0.0, 25.5),
            &[12.5],
            &GuardrailLimits::default(),
            true,
        )
        .expect("verdicts");
        assert!(!v.ok);
        assert_eq!(v.cells[0].check, CellCheck::UnknownIndex { len: 1 });
    }

    #[test]
    fn too_many_cells_rejects_the_whole_request() {
        let limits = GuardrailLimits {
            max_cells_per_change: 2,
            ..GuardrailLimits::default()
        };
        let edits = vec![(0, 1.0), (1, 1.0), (2, 1.0)];
        let err =
            validate_change(&req(edits), (0.0, 25.5), &[1.0, 1.0, 1.0], &limits, true).unwrap_err();
        assert_eq!(err, GuardrailViolation::TooManyCells { count: 3, max: 2 });
    }

    #[test]
    fn rate_limiter_enforces_min_interval() {
        let limits = GuardrailLimits::default(); // min_interval_ms = 1000
        let mut rl = RateLimiter::default();
        assert!(rl.check(10_000, &limits).is_ok());
        assert_eq!(
            rl.check(10_400, &limits).unwrap_err(),
            GuardrailViolation::RateLimited { wait_ms: 600 }
        );
        assert!(rl.check(11_000, &limits).is_ok());
    }
}
