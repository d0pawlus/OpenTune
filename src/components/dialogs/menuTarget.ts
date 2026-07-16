// SPDX-License-Identifier: GPL-3.0-or-later
import type { DefinitionDto } from "../../ipc/bindings";

export interface MenuTarget {
  kind: "dialog" | "table" | "curve";
  name: string;
}

/**
 * Resolve what a menu item's `dialog` reference actually opens. TunerStudio
 * INIs point menu entries at dialogs, but also directly at table editors,
 * their 3-D map ids (`subMenu = veTableMap`), and curves. Dialogs win on a
 * name clash (the pre-existing behavior); unknown names fall through as a
 * dialog target so `DialogEngine` renders its no-dialog notice.
 */
export function resolveMenuTarget(
  definition: DefinitionDto,
  name: string,
): MenuTarget {
  if (!definition.dialogs.some((d) => d.name === name)) {
    const table = definition.tables.find(
      (t) => t.name === name || t.map3d_id === name,
    );
    if (table) return { kind: "table", name: table.name };
    if (definition.curves.some((c) => c.name === name)) {
      return { kind: "curve", name };
    }
  }
  return { kind: "dialog", name };
}
