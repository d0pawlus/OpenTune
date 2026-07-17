# M6 — Interop, polish & first release: decisions

> Updated 2026-07-18. M0–M5 were complete at M6 entry. This note records the
> accepted M6 scope and the distinction between updater signing and operating
> system publisher signing.

## Accepted completion scope

| Area | Decision | Status |
| --- | --- | --- |
| Compatibility | Exercise Speeduino, rusEFI, and one MegaSquirt family member; publish exact counts, diagnostics, hashes, and reproduction commands | Automated/file evidence complete; disposable cross-app GUI re-save pending |
| Updater | Use the official Tauri updater/process plugins; check without blocking startup and install/restart only after explicit confirmation | Complete |
| Updater identity | Generate one encrypted Tauri updater key outside git; embed only its public key; keep private key/password in GitHub secrets | Complete |
| Packages | Build macOS arm64/x64, Windows x64, and Linux x64 release artifacts and checksums | Workflow complete; tag run is the external proof |
| Apple/Windows publisher signing | Do not block M6 on an Apple Developer account or Windows signing certificate | **Deferred by owner approval; add later** |
| Onboarding/i18n | First-run/reopenable guide; English/Polish and default/high-contrast preferences persisted locally | Complete |
| Accessibility | Automated axe/focus/i18n baseline plus manual keyboard, contrast, and VoiceOver smoke | Automated complete; manual UI smoke pending |
| Documentation | Publish GitHub Pages from `docs/` with quick start, update/recovery, compatibility, accessibility, and status pages | Workflow and local build complete; deployment is verified after merge |

## Signing boundary

The Tauri updater signature protects downloaded update archives and is required
for M6. It is not Apple notarization, Apple Developer ID signing, or Windows
Authenticode publisher signing. Until those publisher credentials are obtained:

- release notes and install documentation disclose Gatekeeper/SmartScreen
  warnings;
- the workflow does not contain placeholder certificates or ad-hoc OS signing;
- later publisher-signed builds retain the same updater trust chain.

## Non-claims and follow-ups

- No physical ECU write/burn safety claim is made from simulator and file tests.
- Honda OBD1 and BMW MS4x remain collaboration candidates; no definitions or
  serial captures were available for this release.
- Linux arm64 remains planned; the M6 build matrix is Linux x64.
- Manual UI acceptance is recorded on the evidence pages and cannot be replaced
  by a synthetic “passed” entry when macOS automation permissions are absent.
- Feedback from issue #13 (touch-first UI, lock screen, paged grid dashboard)
  remains post-M6 product work, independent of the release pipeline.
