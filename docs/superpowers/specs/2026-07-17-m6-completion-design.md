# M6 Completion Design

**Status:** Approved scope; platform signing deferred by the project owner on
2026-07-17.

## Goal

Finish M6 as the first complete, public, self-updating OpenTune release: verify
TunerStudio project interoperability, validate the supported firmware families,
add user-controlled updates and first-run guidance, publish the existing docs as
a site, and complete the English/Polish accessibility pass.

Apple notarization and Windows code signing are explicitly deferred until the
owner obtains the required paid accounts and certificates. M6 must document this
gap accurately; it must not substitute ad-hoc signatures or describe unsigned
artifacts as trusted.

## Current State

- M0-M5 are merged on `main`.
- Public unsigned `v0.1.0` and rolling nightly installers exist for macOS arm64
  and x64, Windows x64, and Linux x64 (`AppImage`, `deb`, and `rpm`).
- Real Speeduino syntax has a committed golden test. The rusEFI and MS1/MS3
  parser fixes are merged, but the M6 compatibility matrix and reproducible
  verification evidence are not yet committed.
- `.msq` import/export exists and has focused unit coverage. A real
  TunerStudio open -> OpenTune edit/save -> TunerStudio reopen acceptance result
  is still missing.
- English and Polish dictionaries have compile-time key parity. Locale choice is
  not persisted, several shell strings bypass the dictionaries, and no complete
  accessibility audit exists.
- There is no updater, onboarding flow, or documentation deployment.
- The Rust baseline is green. The frontend currently passes 294 assertions but
  exits non-zero because deferred `uPlot` microtasks reach an unimplemented
  jsdom canvas after `DatalogPanel` tests finish.

## Scope Decisions

### 1. Compatibility is evidence, not another parser rewrite

M6 adds the smallest reproducible compatibility harness around the parsers that
already exist. For each supported family (Speeduino, rusEFI, and one MS family
member), the repository records:

- source/version and SHA-256 of the tested INI and `.msq` inputs;
- parser outcome and diagnostic counts;
- `.msq` applied/skipped/clamped/failed counts;
- the exact command used to reproduce the check;
- whether the file can be opened, edited, saved, and reopened in OpenTune;
- for the TunerStudio round trip, whether TunerStudio reopens the OpenTune output
  and preserves the deliberately changed scalar, bits value, and table cell.

Redistributable inputs become committed fixtures. Non-redistributable inputs are
identified by source and hash in the compatibility report, while focused derived
fixtures pin every syntax behavior they exposed. M6 does not claim Honda OBD1 or
MS4x support without files and evidence from the issue reporter.

The existing `ini` dump and `project` `msq_dump` examples are reused directly;
the compatibility report records their exact `cargo run --example ...` commands.
No new parser facade, wrapper command, or compatibility framework is introduced.

### 2. Updates are cryptographically signed and user-controlled

The Tauri updater signature is independent of Apple/Windows publisher
certificates and is included in M6. A Tauri updater keypair is generated once:

- the public key is committed in `tauri.conf.json`;
- the encrypted private key and its password are stored as GitHub Actions
  secrets `TAURI_SIGNING_PRIVATE_KEY` and
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, and backed up by the owner;
- the private key is never committed or printed in logs.

Tagged release builds generate updater artifacts and `latest.json`. The app uses
`https://github.com/d0pawlus/OpenTune/releases/latest/download/latest.json`.
A failed check is non-fatal and never blocks offline tuning. When an update
exists, the app shows version and release notes; download/install/restart starts
only after an explicit user action. The same UI also exposes a manual "Check for
updates" action.

`v0.2.0` is published as a non-draft, non-prerelease release so GitHub's
`releases/latest` endpoint resolves it. It is the first M6 release eligible for
the updater endpoint. Its release notes state plainly that macOS and Windows
publisher signatures are deferred. Later signed builds keep the same updater
public key, preserving the update chain.

### 3. Onboarding is one small first-run surface

On first launch, an accessible modal introduces three existing workflows:

1. explore safely with the simulator;
2. open an INI/project and edit offline;
3. connect real hardware only after confirming the firmware definition and
   keeping a recoverable tune backup.

The modal also selects English or Polish and default or high-contrast theme.
The choices and a versioned `onboarding-complete` flag are stored in
`localStorage`; no Rust settings subsystem is added. The app derives the initial
locale from the saved choice, then the browser language, then English. The modal
supports Escape, keeps focus inside while open, restores focus when closed, and
can be reopened from the app footer.

No account, telemetry, sample-data download, multi-page wizard, or cloud setup is
added.

### 4. Documentation site reuses `docs/`

GitHub Pages publishes the existing Markdown through Jekyll. The repository adds
only the minimal Pages configuration, navigation/index pages, and deployment
workflow; it does not add a JavaScript documentation framework.

The site exposes:

- installation and unsigned-build warnings;
- a simulator/offline quick start;
- update behavior and recovery guidance;
- the compatibility matrix and reproduction commands;
- architecture, protocol, INI format, roadmap, and ADR links;
- contribution instructions.

Application onboarding links to the relevant hosted quick-start page through the
already-installed Tauri opener plugin.

### 5. Accessibility and i18n are release gates

All OpenTune-authored visible copy moves through the existing `t()` dictionaries.
Firmware-provided labels, units, and option names stay verbatim. English and
Polish retain compile-time key parity.

The pass covers:

- semantic headings, landmarks, form labels, status/live regions, and table/grid
  names;
- full keyboard access and visible focus;
- focus behavior for onboarding and asynchronous error/update messages;
- high-contrast colors and a reduced-motion fallback;
- touch targets for primary controls without redesigning the dashboard;
- an automated `axe-core` smoke of the application shell and focused semantic
  tests for custom grids/canvas gauges.

`axe-core` is the only new accessibility development dependency. It catches
structural regressions; keyboard tests and manual contrast/screen-reader checks
cover behavior that jsdom cannot prove.

## Component Boundaries

- `src/i18n/`: adds M6 copy and initial-locale resolution; remains the only app
  translation source.
- `src/components/onboarding/`: modal presentation only; receives locale/theme
  values and callbacks.
- `src/components/update/`: updater state and presentation; calls the official
  Tauri updater API and exposes check/install actions.
- `src/App.tsx`: owns persisted preferences and composes the two M6 surfaces. It
  does not absorb updater protocol details.
- `src-tauri/src/lib.rs` and Tauri capabilities/config: register and permit the
  updater/opener operations.
- `.github/workflows/`: release artifacts, updater manifest, Pages deployment,
  and verification gates.
- `docs/compatibility/`: the evidence matrix and exact reproduction record.

No general settings store, service container, update abstraction, documentation
application, or compatibility database is introduced.

## Implementation Slices

M6 is delivered as four independently reviewable slices rather than one broad
change:

1. **Baseline and interoperability:** eliminate the existing frontend unhandled
   errors, build the compatibility evidence, and complete the TunerStudio round
   trip.
2. **Updater and release pipeline:** configure updater signing, integrate the
   official plugin/UI, and produce a verifiable release manifest.
3. **First-run accessibility and i18n:** persist preferences, add onboarding,
   route app copy through the dictionaries, and close the accessibility audit.
4. **Documentation and release closure:** deploy the existing docs, record manual
   acceptance, update status documents, and publish/verify `v0.2.0`.

Each slice gets its own implementation plan and validation checkpoint. Slices 2
and 3 may proceed independently after slice 1 restores a clean baseline; slice 4
consumes the evidence from all earlier slices.

## Data and Error Flow

### Startup

1. Resolve locale/theme from `localStorage` with safe defaults.
2. Render the app immediately; corrupt preference values are ignored.
3. Open onboarding only when its versioned completion flag is absent.
4. After the first render, check for an update without delaying ECU or offline
   functionality.
5. Announce available updates and failures through accessible status regions.

### Update

1. Tauri fetches `latest.json` over HTTPS.
2. The plugin validates version metadata and the artifact signature.
3. No update produces an idle/success state without a modal.
4. An available update is displayed; the user chooses install or later.
5. Signature, network, or install errors remain visible and retryable. The app
   never falls back to an unsigned download.

### Compatibility verification

1. Parse the definition with its project properties/symbols.
2. Load the real `.msq` and record every load-report category.
3. Change one representative scalar, bits value, and table cell.
4. Save and reopen in OpenTune; compare physical values.
5. Reopen the output in TunerStudio for the M6 acceptance record.

Hard parser errors fail the family gate. Recorded, understood diagnostics may be
accepted only when the compatibility report names the unsupported construct and
shows that it does not affect the demonstrated workflow.

## Verification and Release Gates

M6 is complete only when all of the following evidence exists:

1. `npm run lint`, `npm run format:check`, `npm test`, and `npm run build` pass.
2. `npm run rust:fmt`, `npm run rust:clippy`, and `npm run rust:test` pass.
3. The frontend baseline has no unhandled `uPlot`/canvas errors.
4. Speeduino, rusEFI, and MS-family compatibility rows have reproducible evidence.
5. TunerStudio reopens an OpenTune-saved `.msq` with the three representative
   edits intact.
6. Updater tests cover no-update, available-update, install, rejection, and retry;
   a signed artifact/manifest pair is verified in CI.
7. First-run, persistence, keyboard, locale, theme, and reopen-onboarding tests
   pass.
8. Automated accessibility checks pass; manual keyboard, high-contrast, and macOS
   VoiceOver smoke results are recorded.
9. GitHub Pages deploys successfully and every onboarding/docs link returns 200.
10. A tagged `v0.2.0` release contains all supported installers, updater artifacts,
    signatures, `latest.json`, checksums, and explicit unsigned-platform warnings.
11. `docs/ROADMAP.md`, `README.md`, and `docs/notes/m6-decisions.md` describe M6 as
    complete without claiming Apple notarization or Windows publisher signing.

## Explicit Deferrals

- Apple Developer ID signing and notarization.
- Windows publisher code signing and SmartScreen reputation.
- Honda OBD1 and BMW MS4x support until real definitions/captures are supplied.
- Touch lock and paged-grid dashboard proposals from issue #13.
- App Store/Microsoft Store distribution.

The first two are required before a later release may call itself a trusted,
signed 1.0. They do not block the owner-approved unsigned M6 completion.
