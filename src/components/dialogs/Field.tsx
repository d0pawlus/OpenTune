// SPDX-License-Identifier: GPL-3.0-or-later
import { useState } from "react";
import type { ConstantDto, Value } from "../../ipc/bindings";

interface FieldProps {
  /** The constant metadata driving this field (units, limits, kind). */
  constant: ConstantDto;
  /** The current value, or `undefined` before values load. */
  value: Value | undefined;
  /** Whether the field is disabled (e.g. an `enable` condition is false). */
  disabled?: boolean;
  /** Emit a new value to write live. */
  onChange: (value: Value) => void;
}

/**
 * Scalar number input with **commit-on-blur**: the in-progress text lives in
 * local state and the ECU write (`onChange`) fires only on blur or Enter —
 * never per keystroke (per-keystroke live writes would contend with the
 * realtime serial traffic).
 *
 * A `null` scalar is the Task 6 fail-open sentinel (an unresolvable constant
 * reads as `NaN`, which serde_json serializes to JSON `null`) — it renders as
 * an empty "—" display, never as `0`.
 */
function ScalarField({ constant, value, disabled, onChange }: FieldProps) {
  const scalar = value && "Scalar" in value ? value.Scalar : null;
  const canonical =
    scalar === null || scalar === undefined ? "" : String(scalar);

  // `null` = not editing: the input shows the backend value, so a changed
  // `value` prop shows through immediately. While editing, the draft wins
  // until it is committed (blur/Enter) or reverted.
  const [draft, setDraft] = useState<string | null>(null);
  const text = draft ?? canonical;

  const commit = () => {
    if (draft === null) return; // never edited — nothing to write
    setDraft(null); // hand display back to the backend value
    if (draft === canonical) return; // unchanged — no write
    const next = Number(draft);
    if (draft.trim() === "" || Number.isNaN(next)) {
      return; // invalid draft: revert, never write
    }
    onChange({ Scalar: next } as Value);
  };

  const step = constant.digits > 0 ? 10 ** -constant.digits : 1;
  return (
    <label className="field">
      <span className="field-name">{constant.name}</span>
      <span className="field-control">
        <input
          className="field-input"
          type="number"
          aria-label={constant.name}
          value={text}
          placeholder="—"
          min={constant.low ?? undefined}
          max={constant.high ?? undefined}
          step={step}
          disabled={disabled}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === "Enter") commit();
          }}
        />
        {constant.units && (
          <span className="field-units">{constant.units}</span>
        )}
      </span>
    </label>
  );
}

/**
 * A single editable field, bound entirely from data:
 * - `Scalar` → number input clamped to the constant's `low..high`, stepped by
 *   its `digits`, with the unit label; commits on blur/Enter.
 * - `Bits` → a select whose options come from the constant's named bit values.
 * - `Array` / `Text` → a minimal read-only preview (full editors are M4).
 */
export function Field({ constant, value, disabled, onChange }: FieldProps) {
  const { kind } = constant;

  if (kind === "Scalar") {
    return (
      <ScalarField
        constant={constant}
        value={value}
        disabled={disabled}
        onChange={onChange}
      />
    );
  }

  if (typeof kind === "object" && "Bits" in kind && kind.Bits) {
    const options = kind.Bits.options;
    const current = value && "Enum" in value ? value.Enum : 0;
    return (
      <label className="field">
        <span className="field-name">{constant.name}</span>
        <select
          className="field-control field-select"
          aria-label={constant.name}
          value={current}
          disabled={disabled}
          onChange={(e) => onChange({ Enum: Number(e.target.value) } as Value)}
        >
          {options.map((opt, i) => (
            <option key={i} value={i}>
              {opt}
            </option>
          ))}
        </select>
      </label>
    );
  }

  // Array / Text — read-only preview for M2.
  const preview =
    value && "Array" in value && value.Array
      ? value.Array.map((n) => n ?? 0).join(", ")
      : value && "Text" in value && value.Text
        ? value.Text
        : "";
  return (
    <div className="field field-readonly">
      <span className="field-name">{constant.name}</span>
      <span className="field-control field-preview">{preview}</span>
    </div>
  );
}
