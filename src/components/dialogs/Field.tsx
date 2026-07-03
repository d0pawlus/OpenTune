// SPDX-License-Identifier: GPL-3.0-or-later
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
 * A single editable field, bound entirely from data:
 * - `Scalar` → number input clamped to the constant's `low..high`, stepped by
 *   its `digits`, with the unit label.
 * - `Bits` → a select whose options come from the constant's named bit values.
 * - `Array` / `Text` → a minimal read-only preview (full editors are M4).
 */
export function Field({ constant, value, disabled, onChange }: FieldProps) {
  const { kind } = constant;

  if (kind === "Scalar") {
    const current = value && "Scalar" in value ? (value.Scalar ?? 0) : 0;
    const step = constant.digits > 0 ? 10 ** -constant.digits : 1;
    return (
      <label className="field">
        <span className="field-name">{constant.name}</span>
        <span className="field-control">
          <input
            className="field-input"
            type="number"
            aria-label={constant.name}
            value={current}
            min={constant.low ?? undefined}
            max={constant.high ?? undefined}
            step={step}
            disabled={disabled}
            onChange={(e) =>
              onChange({ Scalar: Number(e.target.value) } as Value)
            }
          />
          {constant.units && (
            <span className="field-units">{constant.units}</span>
          )}
        </span>
      </label>
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
