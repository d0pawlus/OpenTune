# M6 Slice 4 — Documentation and Release Closure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Publish the existing documentation through GitHub Pages, make project status truthful, merge the four M6 slices, publish unsigned `v0.2.0`, and verify every M6 release gate.

**Architecture:** GitHub Pages builds the existing `docs/` tree with Jekyll; no documentation application is added. README, roadmap, decisions, compatibility, accessibility, and release notes form the public evidence trail. A non-prerelease `v0.2.0` tag release is the updater's `releases/latest` source. Apple and Windows publisher signatures remain an explicit post-M6 task.

**Tech Stack:** Markdown/Jekyll, GitHub Pages Actions, GitHub CLI, existing CI/release workflows, HTTP verification.

## Global Constraints

- Do not claim macOS notarization, Apple Developer ID signing, Windows Authenticode, SmartScreen reputation, Honda OBD1, MS4x, or physical-hardware validation.
- Do not publish `v0.2.0` until local gates and the branch PR CI are green.
- Never move a published tag. Fixes after publication require a new version.
- Keep Pages content sourced from existing docs; no Node docs framework or duplicated architecture prose.
- All third-party actions in write-capable workflows must be pinned to full commit SHAs.

---

### Task 1: Add the minimal Jekyll Pages surface

**Files:**
- Create: `docs/_config.yml`
- Create: `docs/index.md`
- Create: `docs/quick-start.md`
- Create: `docs/updates.md`
- Create: `.github/workflows/pages.yml`

- [ ] **Step 1: Write the site configuration and landing page**

Configure title `OpenTune`, description, repository URL, `baseurl: /OpenTune`, GitHub Pages-compatible `theme: minima`, and header navigation for Quick start, Compatibility, Architecture, Roadmap, and Contributing. The index links into existing architecture/protocol/INI/ADR documents rather than repeating them.

- [ ] **Step 2: Add quick-start and update/recovery pages**

Quick start covers simulator, offline INI/project, backups, and careful real-hardware connection. Updates explains explicit user approval, signature verification, retry/offline behavior, rollback through Releases, checksums, and the distinction between updater signing and deferred publisher signing.

- [ ] **Step 3: Add a pinned Pages workflow**

Trigger on `main` changes under `docs/**` or the workflow plus manual dispatch. Use read-only contents, write Pages, and OIDC permissions. Pin `checkout`, `configure-pages`, `jekyll-build-pages`, `upload-pages-artifact`, and `deploy-pages` to the full SHAs resolved from their current stable major releases. Build from `./docs` to `./_site`.

- [ ] **Step 4: Build the site locally**

Use the GitHub Pages-compatible Jekyll container or Bundler without committing a JavaScript docs dependency. Verify generated `index.html`, quick-start, updates, compatibility, architecture, and roadmap pages exist and internal links resolve.

### Task 2: Close project status and deferred-signing notes

**Files:**
- Modify: `README.md`
- Modify: `docs/ROADMAP.md`
- Modify: `docs/notes/m6-decisions.md`
- Create: `docs/releases/v0.2.0.md`

- [ ] **Step 1: Update README status and navigation**

Describe M0-M6 as implemented, link the documentation site, compatibility evidence, and v0.2.0 release. Keep the pre-production/real-hardware caution and unsigned macOS/Windows installation warnings.

- [ ] **Step 2: Mark evidence-backed M6 items complete**

Update the M6 roadmap checklist only for gates actually passed. Replace the old combined signing line with:

- updater artifacts cryptographically signed — complete;
- Apple notarization/Developer ID — deferred until the owner obtains an account/certificate;
- Windows publisher signing — deferred until the owner obtains a certificate.

State that the owner approved unsigned M6 completion on 2026-07-17.

- [ ] **Step 3: Reconcile the decisions notebook**

Replace stale blockers and old version assumptions with links to the approved design, four implementation plans, compatibility/a11y reports, Pages URL, updater secret names, and the explicit signing deferrals. Keep the issue #13 response as a draft unless the owner separately authorizes posting it.

- [ ] **Step 4: Write v0.2.0 release notes**

Summarize M6 interoperability, updater, onboarding, accessibility/i18n, docs, supported artifacts, hashes, known unsigned-platform warnings, and exact deferrals. Do not describe the app as production-safe for real ECUs.

### Task 3: Final local verification and documentation commit

- [ ] **Step 1: Run every repository gate**

```bash
npm run lint
npm run format:check
npm test
npm run build
npm run rust:fmt
npm run rust:clippy
npm run rust:test
cargo build --workspace --manifest-path src-tauri/Cargo.toml
git diff --check
```

Expected: every command exits 0 with no unhandled frontend errors.

- [ ] **Step 2: Verify metadata alignment**

Use `jq` and `cargo metadata` to confirm version `0.2.0` in `package.json`, `src-tauri/tauri.conf.json`, and the `opentune` Rust package. Confirm the updater endpoint, public key, capabilities, Pages links, and signing-deferral wording.

- [ ] **Step 3: Commit**

```bash
git add docs README.md .github/workflows/pages.yml
git commit -m "docs(m6): close unsigned release milestone"
```

### Task 4: Publish through a reviewed branch

**Files:**
- GitHub branch/PR metadata only.

- [ ] **Step 1: Push the feature branch and open a PR**

Push `feat/m6-completion`, open a focused PR targeting `main`, link the approved design and four evidence reports/plans, list all validation commands, and call out both deferred platform signatures prominently.

- [ ] **Step 2: Wait for and inspect CI**

Use `gh pr checks --watch`. If any check fails, inspect the exact failed logs, fix on the branch, rerun local focused verification, push, and wait again.

- [ ] **Step 3: Merge only when green**

Merge the PR using the repository's normal merge strategy. Pull/verify the resulting `main` commit before tagging.

### Task 5: Build and publish `v0.2.0`

**Files:**
- Git tag `v0.2.0`
- GitHub release `v0.2.0`

- [ ] **Step 1: Tag the merged commit**

Create annotated tag `v0.2.0` at the verified `main` merge commit and push it. Do not reuse or move an existing published tag.

- [ ] **Step 2: Watch release CI**

Watch the release workflow until all four platform builds and final verification job pass. If a draft exists from a failed first run, preserve the tag and rerun/fix; do not force-move it after publication.

- [ ] **Step 3: Inspect the draft assets**

Require macOS arm64/x64 installers and updater archives, Windows x64 MSI/NSIS plus updater archive, Linux x64 AppImage/deb/rpm plus updater archive where supported, all `.sig` files, `latest.json`, and `SHA256SUMS`. Verify hashes and updater-manifest URLs/signatures.

- [ ] **Step 4: Publish as latest normal release**

Set release notes from `docs/releases/v0.2.0.md`, then publish with `isDraft=false` and `isPrerelease=false`. Confirm `https://github.com/d0pawlus/OpenTune/releases/latest/download/latest.json` returns 200 and version `0.2.0`.

### Task 6: Verify Pages and the final M6 gates

- [ ] **Step 1: Enable/verify Pages workflow source**

Ensure repository Pages uses GitHub Actions. Watch the Pages workflow to success.

- [ ] **Step 2: Probe public documentation links**

Require HTTP 200 for site root, quick start, updates, compatibility report, architecture, protocol, INI format, roadmap, ADR index, and contribution link.

- [ ] **Step 3: Audit the final public state**

Verify:

- main CI green;
- release workflow green;
- Pages workflow green;
- v0.2.0 is latest, public, normal release;
- updater manifest and signatures present;
- checksums match downloaded assets;
- README/roadmap/decisions agree that M6 is complete unsigned;
- deferred Apple/Windows signing tasks are explicit.

- [ ] **Step 4: Mark the active M6 goal complete**

Only after all gates above have evidence, mark the goal complete and report the PR, merge commit, release URL, Pages URL, validation commands, compatibility results, and deferred-signing note to the user.
