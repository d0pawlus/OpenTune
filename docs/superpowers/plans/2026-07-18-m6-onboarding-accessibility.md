# M6 Slice 3 — Onboarding, Accessibility, and i18n Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist locale/theme, add a keyboard-safe bilingual first-run modal, remove app-shell hardcoded copy, and establish automated plus recorded manual accessibility gates.

**Architecture:** A tiny `preferences.ts` module owns validated `localStorage` reads/writes. `App` owns preference state. `Onboarding` is a controlled presentational component with focus handling and a docs link through the already-installed opener plugin. Existing i18n dictionaries remain the only translation source. `axe-core` is the sole accessibility dependency.

**Tech Stack:** React 19, TypeScript, Vitest/Testing Library, axe-core, CSS media queries, Tauri opener plugin.

## Global Constraints

- No general settings store, routing system, telemetry, account, or multi-page wizard.
- Firmware-provided labels/options/units remain untranslated.
- Ignore corrupt `localStorage` values and continue with safe defaults.
- Onboarding must support Escape, cyclic Tab/Shift+Tab, initial focus, and focus restoration.
- All primary controls need a visible `:focus-visible` state and a 44px minimum hit area where layout permits.
- Do not edit generated `src/ipc/bindings.ts`.

---

### Task 1: Persist validated user preferences with tests first

**Files:**
- Create: `src/preferences.test.ts`
- Create: `src/preferences.ts`
- Modify: `src/App.tsx`

**Interfaces:**
- Storage keys: `opentune.locale`, `opentune.theme`, `opentune.onboarding.v1`.
- Exports: `initialLocale()`, `initialTheme()`, `isOnboardingComplete()`, `saveLocale()`, `saveTheme()`, `completeOnboarding()`.

- [ ] **Step 1: Write failing preference tests**

Cover saved EN/PL locale, browser-language fallback, English fallback, valid themes, corrupt values, absent/present onboarding flag, and write functions.

Run: `npm test -- src/preferences.test.ts`

Expected before implementation: fail because the module does not exist.

- [ ] **Step 2: Implement validated localStorage helpers**

Use direct functions and literal unions only. Catch storage access exceptions so disabled/private storage cannot prevent startup. Browser locale resolution order is saved value, `navigator.language` beginning with `pl`, then English.

- [ ] **Step 3: Initialize and persist `App` state**

Replace hardcoded `useState<Locale>("en")` and `useState<Theme>("default")` with lazy initializers. Persist only in the existing locale/theme callbacks, avoiding redundant mount writes.

- [ ] **Step 4: Verify focused tests**

Run: `npm test -- src/preferences.test.ts`

Expected: all cases pass.

### Task 2: Add accessible onboarding with tests first

**Files:**
- Create: `src/components/onboarding/Onboarding.test.tsx`
- Create: `src/components/onboarding/Onboarding.tsx`
- Create: `src/components/onboarding/onboarding.css`
- Modify: `src/App.tsx`
- Modify: `src/i18n/en.ts`, `src/i18n/pl.ts`

**Interfaces:**
- Controlled props: `open`, `locale`, `theme`, `onLocaleChange`, `onThemeChange`, `onComplete`.
- Opens `https://d0pawlus.github.io/OpenTune/quick-start/` using `openUrl()` from the existing opener plugin.

- [ ] **Step 1: Write failing semantic and keyboard tests**

Cover:

- absent when `open=false` and labelled `role="dialog"` when true;
- first actionable element receives focus;
- Tab/Shift+Tab wrap within the dialog;
- Escape calls completion;
- locale and theme buttons call their callbacks;
- quick-start link uses `openUrl()`;
- completion returns focus to the previously focused element.

Run: `npm test -- src/components/onboarding/Onboarding.test.tsx`

Expected before implementation: fail because the component does not exist.

- [ ] **Step 2: Implement one modal surface**

Render three short workflow sections: simulator, offline project, and real-hardware safety. Use a labelled backdrop/dialog, button group semantics for locale/theme, and a single completion button. Implement focus capture/wrap/restoration with refs and one keydown handler; do not add a focus-trap package.

- [ ] **Step 3: Add bilingual copy and compose first-run/reopen behavior**

Add matching translation keys for title, intro, all three workflows, language/theme controls, quick start, complete, and reopen. In `App`, initialize open state from `!isOnboardingComplete()`, persist completion on close, and add a footer button that reopens it without clearing the flag.

- [ ] **Step 4: Verify focused tests**

```bash
npm test -- src/components/onboarding/Onboarding.test.tsx
npm test -- src/preferences.test.ts
```

Expected: all cases pass.

### Task 3: Finish app-shell i18n and accessibility styling

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/i18n/en.ts`, `src/i18n/pl.ts`, `src/i18n/i18n.test.ts`
- Modify: `src/styles/tokens.css`

- [ ] **Step 1: Route remaining shell copy through `t()`**

Replace hardcoded heartbeat, language-switch labels, loading placeholders where meaningful, and footer labels with dictionary keys. Keep app name/version and firmware-provided text literal.

- [ ] **Step 2: Strengthen dictionary tests**

Retain compile-time parity and add a focused runtime assertion that every English/Polish value is non-empty. Assert the new shell/onboarding/updater key families resolve in both locales.

- [ ] **Step 3: Add global keyboard/contrast/motion rules**

Add:

- visible `:focus-visible` outline using semantic tokens;
- minimum 44px block size for buttons, selects, and primary text inputs;
- a reduced-motion media query that reduces transition/animation durations;
- high-contrast border/focus tokens that remain visible on the high-contrast surface;
- a constrained, readable main layout without redesigning feature panels.

- [ ] **Step 4: Verify lint, formatting, and i18n**

```bash
npm run format
npm run lint
npm test -- src/i18n/i18n.test.ts
npm run build
```

### Task 4: Add automated accessibility smoke

**Files:**
- Modify: `package.json`, `package-lock.json`
- Create: `src/App.a11y.test.tsx`

- [ ] **Step 1: Install the only accessibility dependency**

Run: `npm install --save-dev axe-core`

- [ ] **Step 2: Write the shell smoke test**

Mock Tauri IPC/event modules and heavy feature panels at their component boundaries, render the actual `App`, run `axe.run(container)`, and expect zero violations. Run once with first-run onboarding open and once with it completed. Do not disable axe rules globally.

- [ ] **Step 3: Add focused custom-widget assertions where axe cannot infer behavior**

Retain existing canvas gauge accessible-name and table/grid semantic tests; add onboarding keyboard assertions in its own suite rather than duplicating them in axe smoke.

- [ ] **Step 4: Verify accessibility tests**

```bash
npm test -- src/App.a11y.test.tsx src/components/onboarding/Onboarding.test.tsx
npm test
```

Expected: zero axe violations and a clean full Vitest exit.

### Task 5: Manual accessibility acceptance and commit

**Files:**
- Create: `docs/accessibility/m6.md`

- [ ] **Step 1: Keyboard and high-contrast smoke**

In the built app, traverse onboarding and the main shell using only Tab, Shift+Tab, Enter/Space, and Escape. Switch high contrast and confirm focus remains visible. Enable macOS Reduce Motion and confirm onboarding/update surfaces do not rely on animation.

- [ ] **Step 2: macOS VoiceOver smoke**

With VoiceOver, verify the app title, onboarding dialog/title, workflow headings, locale/theme controls, update status/error region, main feature headings, and footer controls are announced in sensible order.

- [ ] **Step 3: Record results**

Document date, macOS/VoiceOver version, tested flow, PASS/FAIL, and any known limitations in `docs/accessibility/m6.md`.

- [ ] **Step 4: Commit**

```bash
git add src/preferences.ts src/preferences.test.ts src/components/onboarding \
  src/App.tsx src/App.a11y.test.tsx src/i18n src/styles/tokens.css \
  package.json package-lock.json docs/accessibility/m6.md
git commit -m "feat(onboarding): add persisted accessible first-run guidance"
```
