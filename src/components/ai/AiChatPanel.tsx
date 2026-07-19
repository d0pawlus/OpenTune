// SPDX-License-Identifier: GPL-3.0-or-later
//
// M7 slice-3 embedded assistant's chat panel: sends user turns via
// `commands.aiSend` and renders the streaming transcript driven by
// `events.aiStreamEvent`. Delta text arrives as many small chunks over the
// life of a turn; appending each one straight to React state would mean a
// re-render per chunk, which contends with the dashboard's rAF gauge loop
// on the same main thread. Deltas are buffered in a `useRef` string instead
// and drained into state on a fixed `DELTA_FLUSH_MS` interval that only
// runs while a turn is in flight (see the `running` effect below); terminal
// events (`done`/`cancelled`/`error`) flush the remainder immediately so no
// text is lost when the interval stops.
//
// Proposal cards: on `proposalReady{id}` we re-fetch the full proposal list
// (`ai_proposals` is a snapshot read, not a diff) and render a card for any
// id not already shown — `seenProposalIds` is the de-dupe so a later
// `proposalReady` for the same id (there isn't one today, but the event
// carries no guarantee) never renders a duplicate card. Cards own their own
// Apply/Dismiss lifecycle (see `ProposalCard`); this panel only decides
// which ones to mount.
import { useCallback, useEffect, useRef, useState } from "react";
import {
  commands,
  events,
  type AiProposalDto,
  type AiStreamEvent,
} from "../../ipc/bindings";
import { t, type Locale } from "../../i18n";
import { ProposalCard } from "./ProposalCard";
import "./ai.css";

export const DELTA_FLUSH_MS = 100;

interface ChatMessageEntry {
  kind: "message";
  role: "user" | "assistant";
  text: string;
}

interface ChatToolEntry {
  kind: "tool";
  name: string;
  ok: boolean;
  summary: string;
}

type ChatEntry = ChatMessageEntry | ChatToolEntry;

function appendAssistantText(
  entries: readonly ChatEntry[],
  text: string,
): ChatEntry[] {
  const last = entries[entries.length - 1];
  if (last && last.kind === "message" && last.role === "assistant") {
    return [...entries.slice(0, -1), { ...last, text: last.text + text }];
  }
  return [...entries, { kind: "message", role: "assistant", text }];
}

export function AiChatPanel({ locale }: { locale: Locale }) {
  const [entries, setEntries] = useState<ChatEntry[]>([]);
  const [input, setInput] = useState("");
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [proposals, setProposals] = useState<AiProposalDto[]>([]);
  const deltaBuffer = useRef("");
  const seenProposalIds = useRef<Set<number>>(new Set());

  const loadNewProposals = useCallback(async () => {
    const result = await commands.aiProposals();
    if (result.status === "error") return;
    const unseen = result.data.filter(
      (p) => !seenProposalIds.current.has(p.id),
    );
    if (unseen.length === 0) return;
    for (const p of unseen) seenProposalIds.current.add(p.id);
    setProposals((prev) => [...prev, ...unseen]);
  }, []);

  const flushBuffer = useCallback(() => {
    const buffered = deltaBuffer.current;
    if (!buffered) return;
    deltaBuffer.current = "";
    setEntries((prev) => appendAssistantText(prev, buffered));
  }, []);

  // Ticks only while a turn is streaming; the cleanup below stops it the
  // instant `running` flips false, so the terminal-event branches in
  // `handleStreamEvent` never need to clear a timer id themselves — they
  // just flush the buffer and let this effect tear itself down.
  useEffect(() => {
    if (!running) return;
    const id = window.setInterval(flushBuffer, DELTA_FLUSH_MS);
    return () => window.clearInterval(id);
  }, [running, flushBuffer]);

  const handleStreamEvent = useCallback(
    (event: AiStreamEvent) => {
      switch (event.kind) {
        case "delta":
          // NEVER setState here: buffered in the ref, drained by the
          // interval (or a terminal-event flush) instead.
          deltaBuffer.current += event.text;
          break;
        case "toolStart":
          // Close out the assistant chunk in progress so the chip this
          // tool call eventually produces lands after it, not mid-sentence.
          flushBuffer();
          break;
        case "toolEnd":
          flushBuffer();
          setEntries((prev) => [
            ...prev,
            {
              kind: "tool",
              name: event.name,
              ok: event.ok,
              summary: event.summary,
            },
          ]);
          break;
        case "proposalReady":
          void loadNewProposals();
          break;
        case "done":
          flushBuffer();
          setRunning(false);
          break;
        case "cancelled":
          flushBuffer();
          setRunning(false);
          break;
        case "error":
          flushBuffer();
          setError(event.message);
          setRunning(false);
          break;
      }
    },
    [flushBuffer, loadNewProposals],
  );

  useEffect(() => {
    const unlisten = events.aiStreamEvent.listen((e) =>
      handleStreamEvent(e.payload),
    );
    return () => {
      unlisten.then((f) => f());
    };
  }, [handleStreamEvent]);

  const handleSend = async () => {
    const text = input.trim();
    if (!text || running) return;
    setEntries((prev) => [...prev, { kind: "message", role: "user", text }]);
    setInput("");
    setError(null);
    setRunning(true);
    const result = await commands.aiSend(text);
    if (result.status === "error") {
      setError(result.error);
      setRunning(false);
    }
  };

  const handleCancel = async () => {
    const result = await commands.aiCancel();
    if (result.status === "error") {
      setError(result.error);
    }
  };

  const handleReset = async () => {
    const result = await commands.aiReset();
    if (result.status === "error") {
      setError(result.error);
      return;
    }
    deltaBuffer.current = "";
    setEntries([]);
    setError(null);
    seenProposalIds.current = new Set();
    setProposals([]);
  };

  return (
    <section className="ai-chat" aria-labelledby="ai-chat-title">
      <header>
        <h2 id="ai-chat-title">{t("ai.chat.title", locale)}</h2>
      </header>

      {error && (
        <p role="alert" className="ai-chat-error">
          {error}
        </p>
      )}

      <div
        className="ai-chat-transcript"
        role="log"
        aria-live="polite"
        aria-busy={running}
      >
        {entries.map((entry, index) =>
          entry.kind === "tool" ? (
            <div key={index} className="ai-chat-tool">
              <span className="ai-chat-tool-name">{entry.name}</span>
              <span
                className={entry.ok ? "ai-chat-tool-ok" : "ai-chat-tool-failed"}
              >
                {entry.ok
                  ? t("ai.chat.toolOk", locale)
                  : t("ai.chat.toolFailed", locale)}
              </span>
              {entry.summary && (
                <span className="ai-chat-tool-summary">{entry.summary}</span>
              )}
            </div>
          ) : (
            <p
              key={index}
              className={`ai-chat-message ai-chat-message--${entry.role}`}
            >
              {entry.role === "user"
                ? `${t("ai.chat.you", locale)}: ${entry.text}`
                : entry.text}
            </p>
          ),
        )}
      </div>

      {proposals.length > 0 && (
        <div className="ai-proposals">
          {proposals.map((proposal) => (
            <ProposalCard
              key={proposal.id}
              proposal={proposal}
              locale={locale}
              // The card already shows its own "Applied" status and the
              // dirty badge comes from the backend's `tune_dirty` event via
              // `setCells` — nothing left for the panel to do here. M8 is
              // the UX milestone; keep this minimal until then.
              onApplied={() => {}}
            />
          ))}
        </div>
      )}

      {running && (
        <p role="status" className="ai-chat-status">
          {t("ai.chat.running", locale)}
        </p>
      )}

      <form
        className="ai-chat-input"
        onSubmit={(e) => {
          e.preventDefault();
          void handleSend();
        }}
      >
        <input
          type="text"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          placeholder={t("ai.chat.placeholder", locale)}
          aria-label={t("ai.chat.placeholder", locale)}
        />
        <div className="ai-chat-actions">
          <button type="submit" disabled={running || input.trim().length === 0}>
            {t("ai.chat.send", locale)}
          </button>
          {running && (
            <button type="button" onClick={() => void handleCancel()}>
              {t("ai.chat.cancel", locale)}
            </button>
          )}
          <button
            type="button"
            disabled={running}
            onClick={() => void handleReset()}
          >
            {t("ai.chat.reset", locale)}
          </button>
        </div>
      </form>
    </section>
  );
}
