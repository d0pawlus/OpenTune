// SPDX-License-Identifier: GPL-3.0-or-later
import { useMemo } from "react";
import type { ConstantDto, DefinitionDto, Value } from "../../ipc/bindings";
import { Field } from "./Field";

/** Guard against a pathological cycle of panels referencing each other. */
const MAX_PANEL_DEPTH = 8;

interface DialogEngineProps {
  /** The full definition (dialogs + constants). */
  definition: DefinitionDto;
  /** The name of the dialog to render. */
  dialogName: string;
  /** Current values keyed by constant name. */
  values: Record<string, Value>;
  /**
   * Evaluated `visible`/`enable` expressions, keyed by the raw expression
   * string. The backend `eval_conditions` command is the sole evaluator (one
   * source of truth — no TS port of the expression grammar); a missing entry is
   * treated as visible/enabled (fail-open).
   */
  conditions: Record<string, boolean>;
  /** Emit a live edit `(constantName, value)`. */
  onEdit: (name: string, value: Value) => void;
  /** Recursion depth for nested panels. */
  depth?: number;
}

/**
 * Renders a {@link DefinitionDto} dialog purely from data: each field is a
 * bound `Field`, a nested `Panel` (recursion), a static `Label`, or a `Gap`.
 * Fields whose `visible` expression evaluates false are omitted; fields whose
 * `enable` expression evaluates false render disabled.
 */
export function DialogEngine({
  definition,
  dialogName,
  values,
  conditions,
  onEdit,
  depth = 0,
}: DialogEngineProps) {
  const constants = useMemo(
    () =>
      new Map<string, ConstantDto>(
        definition.constants.map((c) => [c.name, c]),
      ),
    [definition],
  );

  const dialog = definition.dialogs.find((d) => d.name === dialogName);
  if (!dialog || depth > MAX_PANEL_DEPTH) {
    return null;
  }

  const passes = (expr: string | null): boolean =>
    expr === null || conditions[expr] !== false;

  return (
    <section className="dialog" aria-label={dialog.title}>
      {depth === 0 && <h3 className="dialog-title">{dialog.title}</h3>}
      <div className="dialog-fields">
        {dialog.fields.map((field, i) => {
          if (!passes(field.visible)) {
            return null;
          }
          const enabled = passes(field.enable);
          const { kind } = field;

          if (typeof kind === "object" && "Constant" in kind && kind.Constant) {
            const name = kind.Constant;
            const constant = constants.get(name);
            if (!constant) {
              return (
                <p key={i} className="field-missing">
                  {name}
                </p>
              );
            }
            return (
              <Field
                key={i}
                constant={constant}
                value={values[constant.name]}
                disabled={!enabled}
                onChange={(v) => onEdit(constant.name, v)}
              />
            );
          }

          if (typeof kind === "object" && "Panel" in kind && kind.Panel) {
            return (
              <DialogEngine
                key={i}
                definition={definition}
                dialogName={kind.Panel}
                values={values}
                conditions={conditions}
                onEdit={onEdit}
                depth={depth + 1}
              />
            );
          }

          if (typeof kind === "object" && "Label" in kind && kind.Label) {
            return (
              <p key={i} className="dialog-label">
                {kind.Label}
              </p>
            );
          }

          // "Gap"
          return <div key={i} className="dialog-gap" aria-hidden="true" />;
        })}
      </div>
    </section>
  );
}
