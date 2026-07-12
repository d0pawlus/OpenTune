# M6 Slice 1 — Unsigned Release Artifacts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `v*` tag push produces a **draft** GitHub pre-release with unsigned installers for macOS (arm64 + x86_64), Windows, and Linux (AppImage + deb), plus tester-facing install notes in the README.

**Architecture:** One new workflow (`release.yml`) using `tauri-apps/tauri-action@v1` on a 4-way OS matrix, reusing the exact dependency-install idioms of the existing `ci.yml`. No signing, no notarization, no auto-update — those are later M6 slices (blocked on user credentials). The draft release is the publish gate: nothing becomes public until the repo owner clicks "Publish".

**Tech Stack:** GitHub Actions, `tauri-apps/tauri-action@v1`, Tauri v2 bundler (config already active in `src-tauri/tauri.conf.json`: `targets: "all"`, icons present, sample INI bundled as resource).

## Global Constraints

- Tauri v2; app version comes from `src-tauri/tauri.conf.json` → `"version": "0.1.0"` (already aligned with `package.json` and `src-tauri/Cargo.toml`).
- Package manager is **npm** (`npm ci`), Node 20 — same as `ci.yml`.
- Linux build deps must include `libudev-dev` (serialport crate) on top of the standard Tauri set — `ci.yml` already proves this list.
- Linux runner pinned to `ubuntu-22.04` (not `-latest`): older glibc baseline → artifacts run on older distros (direct ask from issue #13).
- Release must be created as **draft + prerelease**; publishing is a manual user action.
- Commit messages: conventional commits, no attribution footer (user global setting).

---

### Task 1: Local bundle smoke (no code changes)

**Files:**
- None created/modified. Read-only verification of `src-tauri/tauri.conf.json` bundle config.

**Interfaces:**
- Consumes: existing `npm run tauri build` script and bundle config.
- Produces: confidence that the bundler config is valid before spending CI minutes; artifact paths under `src-tauri/target/release/bundle/`.

- [ ] **Step 1: Run a local release build on macOS**

Run: `npm run tauri build`
Expected: exits 0; takes several minutes (full release compile of the workspace).

- [ ] **Step 2: Verify artifacts exist**

Run: `ls src-tauri/target/release/bundle/dmg/ src-tauri/target/release/bundle/macos/`
Expected: `OpenTune_0.1.0_aarch64.dmg` and `OpenTune.app` are listed.

- [ ] **Step 3: Launch the bundled app once**

Run: `open src-tauri/target/release/bundle/macos/OpenTune.app`
Expected: the app window opens, the Connect panel renders (sample INI resource resolves in the bundled layout — this is the one thing dev mode does not exercise).

If the resource fails to resolve, stop: fix `bundle.resources` pathing in `src-tauri/tauri.conf.json` before continuing (that would be a real config bug this task exists to catch).

### Task 2: `release.yml` workflow

**Files:**
- Create: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: repo build scripts (`npm ci`, tauri CLI via `tauri-action`), `src-tauri/tauri.conf.json` version.
- Produces: on `v*` tag push — a draft prerelease named `OpenTune <tag>` with assets: 2× `.dmg` + 2× `.app.tar.gz` (mac arm/intel), `.msi` + NSIS `.exe` (Windows), `.AppImage` + `.deb` (Linux). Tag convention `v*` is what Task 4 and future auto-update slices rely on.

- [ ] **Step 1: Write the workflow file**

```yaml
name: Release

on:
  push:
    tags:
      - 'v*'

jobs:
  build-release:
    permissions:
      contents: write
    strategy:
      fail-fast: false
      matrix:
        include:
          - platform: macos-latest # Apple Silicon
            args: '--target aarch64-apple-darwin'
          - platform: macos-latest # Intel
            args: '--target x86_64-apple-darwin'
          - platform: ubuntu-22.04 # older glibc baseline for wider distro compat
            args: ''
          - platform: windows-latest
            args: ''
    runs-on: ${{ matrix.platform }}
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with: { node-version: 20, cache: npm }
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.platform == 'macos-latest' && 'aarch64-apple-darwin,x86_64-apple-darwin' || '' }}
      - name: Install Linux Tauri deps
        if: matrix.platform == 'ubuntu-22.04'
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libudev-dev xdg-utils
      - run: npm ci
      - uses: tauri-apps/tauri-action@v1
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        with:
          tagName: ${{ github.ref_name }}
          releaseName: 'OpenTune ${{ github.ref_name }}'
          releaseBody: |
            Unsigned pre-release for testing. See the README "Download (pre-release)"
            section for per-OS install notes (Gatekeeper / SmartScreen warnings are expected).
          releaseDraft: true
          prerelease: true
          args: ${{ matrix.args }}
```

- [ ] **Step 2: Validate the YAML parses**

Run: `ruby -ryaml -e 'YAML.load_file(".github/workflows/release.yml"); puts "ok"'`
Expected: `ok` (macOS ships system Ruby; no new dev dependency).

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: build draft pre-release artifacts on v* tags"
```

### Task 3: README tester install notes

**Files:**
- Modify: `README.md` (add a `## Download (pre-release)` section after the project intro)

**Interfaces:**
- Consumes: release asset names produced by Task 2.
- Produces: the section the release body links to; copy for the issue #13 reply.

- [ ] **Step 1: Add the section**

```markdown
## Download (pre-release)

Unsigned test builds are published on the
[Releases page](https://github.com/d0pawlus/TuningSoftware/releases).
These are pre-1.0 builds for testing — expect OS warnings:

- **macOS** — the app is not notarized yet. Right-click the app → *Open*
  (once), or clear the quarantine flag: `xattr -cr /Applications/OpenTune.app`.
- **Windows** — SmartScreen will warn about an unknown publisher.
  *More info* → *Run anyway*.
- **Linux** — download the `.AppImage`, then `chmod +x OpenTune_*.AppImage`
  and run it. A `.deb` is also provided for Debian/Ubuntu. Serial-port access
  may require adding your user to the `dialout` group
  (`sudo usermod -aG dialout $USER`, then re-login).

Signed and notarized builds are planned before 1.0 (see
[ROADMAP — M6](docs/ROADMAP.md)).
```

- [ ] **Step 2: Verify the markdown renders sanely**

Run: `npx --yes markdownlint-cli2 README.md || true` (advisory only — repo has no markdown linter; a visual check in the GitHub preview after push is the real gate)
Expected: no new hard errors introduced by the section.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: add pre-release download and install notes"
```

### Task 4: Tag, watch CI, verify the draft release

**Files:**
- None. Git tag + verification only.

**Interfaces:**
- Consumes: Task 2 workflow, Task 3 README.
- Produces: draft release `OpenTune v0.1.0` with 8 assets, ready for the user to publish and to answer issue #13.

- [ ] **Step 1: Push the tag**

```bash
git tag v0.1.0
git push origin main v0.1.0
```

- [ ] **Step 2: Watch the workflow**

Run: `gh run watch $(gh run list --workflow=release.yml --limit 1 --json databaseId --jq '.[0].databaseId')`
Expected: all 4 matrix jobs green. Windows job only *builds* (no tests run here), so the known `opentune --lib` Windows test issue does not gate this workflow.

- [ ] **Step 3: Verify the draft release assets**

Run: `gh release view v0.1.0 --json isDraft,assets --jq '{draft: .isDraft, assets: [.assets[].name]}'`
Expected: `draft: true`; assets include `OpenTune_0.1.0_aarch64.dmg`, `OpenTune_0.1.0_x64.dmg`, two `.app.tar.gz`, `OpenTune_0.1.0_x64_en-US.msi`, `OpenTune_0.1.0_x64-setup.exe`, `OpenTune_0.1.0_amd64.AppImage`, `OpenTune_0.1.0_amd64.deb` (exact names may vary slightly by bundler version — the check is: all four OS/arch families are present).

- [ ] **Step 4 (USER, not agent): Publish the draft + reply to issue #13**

Publishing the release and posting the issue reply are outward-facing actions reserved for the repo owner. A drafted reply is prepared in `docs/notes/m6-decisions.md`.

**If a matrix job fails:** fix on `main`, delete the draft release (`gh release delete v0.1.0`), move the tag (`git tag -f v0.1.0 && git push -f origin v0.1.0`). Draft releases are invisible to the public, so force-moving this tag is safe until first publish; after the first *published* release, never move tags — bump the version instead.
