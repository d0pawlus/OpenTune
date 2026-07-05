// SPDX-License-Identifier: GPL-3.0-or-later
// Dashboard layout (de)serialization — the JSON blob persisted by the
// backend `save_layout`/`load_layout` commands. Pure and unit-tested; the
// Dashboard component only orchestrates.

import type { FrontPageDto } from "../../ipc/bindings";

/** How a slot renders its bound gauge. */
export type GaugeKind = "round" | "bar" | "digital";

/** One dashboard slot: a gauge binding plus its display kind. */
export interface SlotLayout {
  gauge: string;
  kind: GaugeKind;
}

const KINDS: readonly GaugeKind[] = ["round", "bar", "digital"];

export const LAYOUT_VERSION = 1;

/** The INI `[FrontPage]` defaults: its gauge slots, rendered as round gauges. */
export function defaultSlots(frontpage: FrontPageDto): SlotLayout[] {
  return frontpage.gauge_slots.map((gauge) => ({
    gauge,
    kind: "round" as const,
  }));
}

/** Serialize slots to the persisted JSON shape. */
export function serializeLayout(slots: readonly SlotLayout[]): string {
  return JSON.stringify({ version: LAYOUT_VERSION, slots });
}

/**
 * Parse persisted layout JSON. Never trusts file content: malformed JSON or
 * an unexpected shape returns `null` (the caller falls back to the INI
 * `[FrontPage]`); slots naming gauges that no longer exist in the definition
 * are dropped; unknown kinds degrade to `"round"`.
 */
export function parseLayout(
  json: string,
  validGaugeNames: readonly string[],
): SlotLayout[] | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch {
    return null;
  }
  if (typeof parsed !== "object" || parsed === null) return null;
  const slots = (parsed as { slots?: unknown }).slots;
  if (!Array.isArray(slots)) return null;

  const valid = new Set(validGaugeNames);
  const result: SlotLayout[] = [];
  for (const entry of slots) {
    if (typeof entry !== "object" || entry === null) continue;
    const { gauge, kind } = entry as { gauge?: unknown; kind?: unknown };
    if (typeof gauge !== "string" || !valid.has(gauge)) continue;
    result.push({
      gauge,
      kind: KINDS.includes(kind as GaugeKind) ? (kind as GaugeKind) : "round",
    });
  }
  return result;
}

/**
 * Return a new array with the slot at `index` swapped one place up or down.
 * Out-of-range moves return an unchanged copy; the input is never mutated.
 */
export function moveSlot(
  slots: readonly SlotLayout[],
  index: number,
  delta: -1 | 1,
): SlotLayout[] {
  const target = index + delta;
  const next = [...slots];
  if (
    index < 0 ||
    index >= slots.length ||
    target < 0 ||
    target >= slots.length
  ) {
    return next;
  }
  [next[index], next[target]] = [next[target], next[index]];
  return next;
}
