// SPDX-License-Identifier: GPL-3.0-or-later
import { en } from "./en";
import { pl } from "./pl";

export type Locale = "en" | "pl";
const dicts = { en, pl } as const;

export function t(key: keyof typeof en, locale: Locale = "en"): string {
  return dicts[locale][key] ?? key;
}
