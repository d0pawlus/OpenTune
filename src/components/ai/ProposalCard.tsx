// SPDX-License-Identifier: GPL-3.0-or-later
//
// M7 slice-3 task 6 — the advisory-level payoff: the assistant can only ever
// *propose* a change (`opentune-ai`'s `propose_change` tool, guardrail-
// checked on the backend); the USER applies it, explicitly, by clicking this
// card's Apply button. There is no code path from a `proposalReady` stream
// event to a write — `AiChatPanel` only renders this card, and Apply is the
// sole trigger for `setCells`.
//
// Apply reuses `useTuneStore`'s `setCells` — the exact optimistic-update +
// rollback-and-rethrow gesture `AutoTunePanel.apply` already uses for the
// same backend command, so a proposal write behaves identically to an
// autotune write (dirty badge, rollback on failure, one undo step).
import { useState } from "react";
import type { AiProposalDto, CellEditDto } from "../../ipc/bindings";
import { useTuneStore } from "../../stores/tune";
import type { CellEdit } from "../table-editor/tableOps";
import { t, type Locale } from "../../i18n";
import "./ai.css";

interface ProposalCardProps {
  proposal: AiProposalDto;
  locale: Locale;
  onApplied: () => void;
}

// `CellEditDto.value` is `number | null` only because specta-typescript
// projects every backend `f64` that way (see `AutoTunePanel`'s `num()`
// comment) — `edits` itself is only ever populated when the proposal is
// `ok`, so every entry carries a real value in practice. A null is dropped
// rather than defaulted to 0: silently writing an unintended 0 to a live
// tuning constant is worse than skipping a cell that can't be applied.
function toCellEdits(edits: readonly CellEditDto[]): CellEdit[] {
  return edits.flatMap((e) =>
    e.value === null ? [] : [{ index: e.index, value: e.value }],
  );
}

export function ProposalCard({
  proposal,
  locale,
  onApplied,
}: ProposalCardProps) {
  const [applying, setApplying] = useState(false);
  const [applied, setApplied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState(false);

  if (dismissed) return null;

  const canApply = proposal.ok && proposal.edits.length > 0;

  const handleApply = async () => {
    setError(null);
    setApplying(true);
    try {
      await useTuneStore
        .getState()
        .setCells(proposal.constant, toCellEdits(proposal.edits));
      setApplied(true);
      onApplied();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setApplying(false);
    }
  };

  return (
    <div className="ai-proposal" aria-label={t("ai.proposal.title", locale)}>
      <header className="ai-proposal-header">
        <span className="ai-proposal-title">
          {t("ai.proposal.title", locale)}
        </span>
        <span className="ai-proposal-constant">{proposal.constant}</span>
      </header>

      <p className="ai-proposal-reason">{proposal.reason}</p>

      <ul className="ai-proposal-cells">
        {proposal.cells.map((cell) => (
          <li key={cell.index} className="ai-proposal-cell">
            <span className="ai-proposal-cell-index">#{cell.index}</span>
            <span className="ai-proposal-cell-value">{cell.value ?? "—"}</span>
            <span
              className={
                cell.ok ? "ai-proposal-cell-ok" : "ai-proposal-cell-note"
              }
            >
              {cell.ok ? "✓" : cell.note}
            </span>
          </li>
        ))}
      </ul>

      {!proposal.ok && (
        <p className="ai-proposal-invalid">
          {t("ai.proposal.invalid", locale)}
        </p>
      )}

      {error && (
        <p role="alert" className="ai-proposal-error">
          {error}
        </p>
      )}

      {applied && (
        <p role="status" className="ai-proposal-applied">
          {t("ai.proposal.applied", locale)}
        </p>
      )}

      <div className="ai-proposal-actions">
        <button
          type="button"
          disabled={!canApply || applying || applied}
          onClick={() => void handleApply()}
        >
          {t("ai.proposal.apply", locale)}
        </button>
        <button type="button" onClick={() => setDismissed(true)}>
          {t("ai.proposal.dismiss", locale)}
        </button>
      </div>
    </div>
  );
}
