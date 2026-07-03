// SPDX-License-Identifier: GPL-3.0-or-later
import type { ConstantDto, TableDto, Value } from "../../ipc/bindings";

interface TableFieldProps {
  /** The table definition (bin/cell constant references). */
  table: TableDto;
  /** Constant metadata by name (to size the grid from the Z array shape). */
  constants: Record<string, ConstantDto>;
  /** Current values by constant name. */
  values: Record<string, Value>;
}

/**
 * Minimal, read-only preview of a table's Z (cell) values laid out on a grid
 * sized by the Z constant's array shape. The full interactive map editor is
 * M4; this exists so a definition's `[TableEditor]` entries are visible in M2.
 */
export function TableField({ table, constants, values }: TableFieldProps) {
  const zConst = constants[table.z];
  const zValue = values[table.z];
  const cells =
    zValue && "Array" in zValue && zValue.Array
      ? zValue.Array.map((n) => n ?? 0)
      : [];

  const zKind = zConst?.kind;
  const cols =
    typeof zKind === "object" && "Array" in zKind && zKind.Array
      ? Math.max(1, zKind.Array.cols)
      : Math.max(1, cells.length);

  return (
    <section className="table-field" aria-label={table.name}>
      <h4 className="table-title">{table.name}</h4>
      {cells.length === 0 ? (
        <p className="table-empty">{table.z}</p>
      ) : (
        <div
          className="table-grid"
          role="grid"
          style={{ gridTemplateColumns: `repeat(${cols}, 1fr)` }}
        >
          {cells.map((cell, i) => (
            <span key={i} role="gridcell" className="table-cell">
              {cell}
            </span>
          ))}
        </div>
      )}
    </section>
  );
}
