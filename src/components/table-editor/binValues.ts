// SPDX-License-Identifier: GPL-3.0-or-later
// Small pure helpers for reading a bound `ConstantDto`/`Value` pair (bin
// count, axis labels, numeric extraction), shared by both editors. Born in
// Task 6 as `curve-editor/binValues.ts` (deliberately duplicating
// `TableEditor.tsx`'s then-private helpers, which were out of that task's
// scope); Task 7 touches `TableEditor` legitimately, so the duplication is
// resolved by moving the module here — the dependency direction stays
// one-way (curve-editor imports table-editor machinery, never the reverse).
import type { ConstantDto, Value } from "../../ipc/bindings";

/** Total element count of an Array-kinded constant, or null for any other. */
export function arrayLength(c: ConstantDto | undefined): number | null {
  const kind = c?.kind;
  if (kind && typeof kind === "object" && "Array" in kind && kind.Array) {
    return kind.Array.rows * kind.Array.cols;
  }
  return null;
}

/** A bound array `Value`'s raw elements, or `null` for anything else. */
export function arrayOf(value: Value | undefined): (number | null)[] | null {
  return value && "Array" in value && value.Array ? value.Array : null;
}

/** Formats a bin array into axis labels with the bin constant's digits. */
export function labelsOf(
  arr: (number | null)[] | null,
  digits: number,
): string[] {
  return (arr ?? []).map((v) =>
    v !== null && Number.isFinite(v) ? v.toFixed(digits) : "—",
  );
}

/** The backend's `null` (NaN) sentinel mapped to `NaN` for the Task 4 ops. */
export function numericOf(arr: (number | null)[] | null): number[] {
  return (arr ?? []).map((v) => (v === null ? NaN : v));
}
