// SPDX-License-Identifier: GPL-3.0-or-later
import { useMemo } from "react";
import type { ConstantDto, DefinitionDto, Value } from "../../ipc/bindings";
import type { Locale } from "../../i18n";
import { Field } from "./Field";
import { resolveMenuTarget } from "./menuTarget";
import { TableEditorView } from "../table-editor/TableEditor";
import { CurveEditorView } from "../curve-editor/CurveEditor";

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
  /** UI locale for embedded table/curve editors. */
  locale?: Locale;
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
  locale = "en",
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
            // A `panel =` may name another dialog OR a table/curve editor
            // (by name or 3-D map id) — rusEFI embeds its VE/ignition
            // tables in dialogs this way. Same resolution rule as menu
            // items: `resolveMenuTarget` is the single copy of the
            // dialogs-win precedence.
            const target = resolveMenuTarget(definition, kind.Panel);
            if (target.kind === "table") {
              const table = definition.tables.find(
                (tb) => tb.name === target.name,
              );
              return table ? (
                <TableEditorView
                  key={i}
                  table={table}
                  constants={definition.constants}
                  locale={locale}
                />
              ) : null;
            }
            if (target.kind === "curve") {
              const curve = definition.curves.find(
                (c) => c.name === target.name,
              );
              return curve ? (
                <CurveEditorView
                  key={i}
                  curve={curve}
                  constants={definition.constants}
                  locale={locale}
                />
              ) : null;
            }
            return (
              <DialogEngine
                key={i}
                definition={definition}
                dialogName={target.name}
                values={values}
                conditions={conditions}
                onEdit={onEdit}
                depth={depth + 1}
                locale={locale}
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
