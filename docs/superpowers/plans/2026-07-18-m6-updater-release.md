# M6 Slice 2 — Updater and Release Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add cryptographically signed, user-controlled Tauri updates and make tagged releases publish updater artifacts, signatures, `latest.json`, and checksums without Apple or Windows publisher signing.

**Architecture:** Use the official Tauri updater and process plugins. `UpdateNotice` owns updater protocol/UI state; `App` only composes it. The Tauri updater public key and GitHub latest-release endpoint live in `tauri.conf.json`. The existing release matrix creates one draft, passes its ID to the pinned official `tauri-action`, then a final job verifies the updater payload and adds checksums. Platform publisher signing remains absent and explicitly documented.

**Tech Stack:** Tauri v2 updater/process plugins, React 19, Vitest, GitHub Actions, `tauri-apps/tauri-action` v1, minisign-compatible Tauri updater keys.

## Global Constraints

- The updater private key and password must never enter git, command output, or release assets.
- Store secrets as `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
- The updater signature is not Apple notarization or Windows Authenticode; retain unsigned-platform warnings.
- Update checks and failures are non-blocking. Download/install/relaunch require an explicit button press.
- Use the pinned action SHA already present in the repository.
- Do not edit generated `src/ipc/bindings.ts`.

---

### Task 1: Provision the updater signing identity

**Files:**
- External secret: `~/Library/Application Support/OpenTune/release/updater.key`
- External public key: `~/Library/Application Support/OpenTune/release/updater.key.pub`
- GitHub Actions secrets: `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

- [ ] **Step 1: Generate a high-entropy password without printing it**

Generate 48 random bytes as base64 in a shell variable. Store it in macOS Keychain under service `org.opentune.updater`, account equal to the local username.

- [ ] **Step 2: Generate the encrypted Tauri updater keypair**

Create the release directory with owner-only permissions and run:

```bash
npm run tauri signer generate -- --ci -p "$updater_password" \
  -w "$updater_key_path"
```

Expected: encrypted private key and `.pub` file exist; neither is under the repository.

- [ ] **Step 3: Set GitHub Actions secrets through stdin**

Pipe the private-key file into `gh secret set TAURI_SIGNING_PRIVATE_KEY`; pipe the shell-held password into `gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Do not echo values.

- [ ] **Step 4: Verify only secret names and local permissions**

Run `gh secret list` and `ls -l` on the external key directory. Expected: both secret names exist and the local directory/key are readable only by the owner. Record the backup location in `docs/notes/m6-decisions.md` without recording secret material.

### Task 2: Configure the official Tauri updater

**Files:**
- Modify: `package.json`, `package-lock.json`
- Modify: `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/capabilities/default.json`
- Modify: `src-tauri/tauri.conf.json`

- [ ] **Step 1: Install the official dependencies**

Run:

```bash
npm install @tauri-apps/plugin-updater@^2 @tauri-apps/plugin-process@^2
cargo add tauri-plugin-updater@2 tauri-plugin-process@2 \
  --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 2: Register Rust plugins and capabilities**

Add `.plugin(tauri_plugin_updater::Builder::new().build())` and `.plugin(tauri_plugin_process::init())` to the existing Tauri builder. Add `updater:default` and `process:allow-restart` to the main-window capability.

- [ ] **Step 3: Configure signed updater artifacts**

Set `plugins.updater.pubkey` to the exact single-line contents of the generated
`updater.key.pub`, then add the endpoint below:

```json
"bundle": {
  "createUpdaterArtifacts": true
},
"plugins": {
  "updater": {
    "endpoints": [
      "https://github.com/d0pawlus/OpenTune/releases/latest/download/latest.json"
    ]
  }
}
```

Merge these keys into the current config without losing bundle targets, resources, icons, CSP, or window config.

- [ ] **Step 4: Validate configuration and compilation**

Run:

```bash
jq empty src-tauri/tauri.conf.json src-tauri/capabilities/default.json
npm run build
cargo check --workspace --manifest-path src-tauri/Cargo.toml
```

Expected: all commands exit 0.

### Task 3: Build the user-controlled update surface with tests first

**Files:**
- Create: `src/components/update/UpdateNotice.test.tsx`
- Create: `src/components/update/UpdateNotice.tsx`
- Create: `src/components/update/update.css`
- Modify: `src/App.tsx`
- Modify: `src/i18n/en.ts`, `src/i18n/pl.ts`

**Interfaces:**
- Props: `locale: Locale`.
- Uses `check()` from `@tauri-apps/plugin-updater`.
- Uses `relaunch()` from `@tauri-apps/plugin-process` only after `downloadAndInstall()` resolves.

- [ ] **Step 1: Write failing behavior tests**

Mock both Tauri plugins and cover:

1. startup check returns `null` and keeps the compact manual-check button;
2. available update shows target version and release notes;
3. install calls `downloadAndInstall()` and then `relaunch()` only after user click;
4. rejected check renders a polite alert and Retry;
5. Retry invokes `check()` again and can recover.

Run: `npm test -- src/components/update/UpdateNotice.test.tsx`

Expected before implementation: fail because the component does not exist.

- [ ] **Step 2: Implement the smallest state machine**

Use local state only: `checking`, `idle`, `available`, `installing`, and `error`. Keep the returned update handle in state so install uses the verified object. Catch all check/install failures, expose retry, and never redirect to an unsigned download.

The available notice uses `role="status"`; errors use `role="alert"`; buttons expose disabled/busy state while asynchronous work is active.

- [ ] **Step 3: Add bilingual updater copy and compose it**

Add matching EN/PL keys for manual check, checking, available version, notes, install/restart, later, retry, no update, and error. Render `<UpdateNotice locale={locale} />` near the application header without blocking any ECU/offline panel.

- [ ] **Step 4: Verify focused tests and format**

```bash
npm test -- src/components/update/UpdateNotice.test.tsx
npm run format
npm run lint
npm run build
```

Expected: all commands exit 0.

### Task 4: Make release CI generate and verify updater assets

**Files:**
- Modify: `.github/workflows/release.yml`

- [ ] **Step 1: Export one release ID from `prepare`**

Give the release-creation step an ID, emit `release_id` through `$GITHUB_OUTPUT`, and expose it as a `prepare` job output. Keep re-runs idempotent by reusing an existing release with the same tag.

Create the draft as a normal release (`--draft` without `--prerelease`) so final publication can become GitHub's latest release without changing asset identity.

- [ ] **Step 2: Let the pinned official action upload signed updater artifacts**

Replace the custom artifact upload step with the existing pinned `tauri-action` configured with:

```yaml
env:
  GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
  TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
with:
  releaseId: ${{ needs.prepare.outputs.release_id }}
  tagName: ${{ github.ref_name }}
  releaseDraft: true
  prerelease: false
  uploadUpdaterJson: true
  uploadUpdaterSignatures: true
  updaterJsonPreferNsis: true
  args: ${{ matrix.args }}
```

Use `releaseAssetNamePattern` only if the focused CI dry-run proves the two macOS updater archives collide; prefer the action's native naming first.

- [ ] **Step 3: Add a final updater/checksum verification job**

After all matrix legs succeed, query the draft release and fail unless it contains installers for macOS arm64/x64, Windows x64, Linux x64, updater archives, `.sig` files, and `latest.json`. Download `latest.json`, assert with `jq` that version is `0.2.0` and that darwin arm64/x64, linux x86_64, and windows x86_64 entries have HTTPS URLs plus non-empty signatures.

Download all release assets except an existing `SHA256SUMS`, run `shasum -a 256`, and upload one deterministic `SHA256SUMS` asset with `--clobber`.

- [ ] **Step 4: Parse and inspect the workflow**

```bash
ruby -ryaml -e 'YAML.load_file(".github/workflows/release.yml"); puts "ok"'
git diff --check
```

Expected: `ok` and no whitespace errors.

### Task 5: Commit and locally verify the slice

- [ ] **Step 1: Run the complete local gate**

```bash
npm run lint
npm run format:check
npm test
npm run build
npm run rust:fmt
npm run rust:clippy
npm run rust:test
```

- [ ] **Step 2: Commit**

```bash
git add package.json package-lock.json src-tauri/Cargo.toml src-tauri/Cargo.lock \
  src-tauri/src/lib.rs src-tauri/capabilities/default.json \
  src-tauri/tauri.conf.json src/components/update src/App.tsx src/i18n \
  .github/workflows/release.yml docs/notes/m6-decisions.md
git commit -m "feat(updater): add signed user-controlled updates"
```
