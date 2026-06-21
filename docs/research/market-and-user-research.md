# Market & user research

This document captures findings from a research pass (2026-06-21) into the
TunerStudio ecosystem: what users actually struggle with, who else is building in
this space, and the technical/legal terrain. It exists so those findings inform
the roadmap rather than being lost. It is **evidence-based** — claims carry source
URLs — and flags where evidence is strong vs. thin.

> Caveats on sources: strongest evidence is directly-fetched **msextra.com**
> threads (verbatim user/developer quotes) and **official TunerStudio/MegaLogViewer
> changelogs** (a fixed bug confirms a real defect). **Reddit was not crawlable**, so
> no r/Speeduino / r/Megasquirt / r/rusEFI sentiment is captured. Some
> speeduino.com / miataturbo.net pages returned HTTP 403, so a few items rest on
> search-index excerpts and are flagged inline.

---

## 1. The competitive landscape (the headline)

**A project nearly identical to OpenTune already exists and is active:**
[**LibreTune**](https://github.com/RallyPat/LibreTune) — Rust + Tauri +
React/TypeScript, GPL-2.0, cross-platform, parses `.ini`/`.msq`/`.mlg`, does live
serial tuning (gauges, table editing, burn-to-ECU, AutoTune), supports
Speeduino/rusEFI/FOME/epicEFI + MS2/MS3 (partial). Created **Jan 2026**, active as
of **Jun 2026**, but **alpha, single-maintainer, ~56 stars, no stable release**.

This is the single most important strategic fact for OpenTune. It is both the
biggest "this already exists" risk and — given its immaturity and bus-factor-of-1
— evidence that the niche is real and still open. The positioning decision is
recorded in [ADR-0007](../adr/0007-positioning-vs-libretune.md): **differentiate**
(build independently, compete on the trust/verification layer).

### LibreTune code-level assessment (verified against the repo)

**Real strengths:** a clean, data-driven INI core separated from the Tauri app; a
working protocol layer with page write **and burn** (not stubs); a production-grade
*realtime* demo simulator and a well-engineered realtime store (Zustand +
`subscribeWithSelector` + off-state circular buffer to avoid 20 Hz re-render
storms).

**The gap — a thin trust/verification layer:**

- **No public firmware/INI corpus**, and **the flash path is untested end-to-end** —
  the simulator covers realtime but **not** page read/write/burn.
- **TS types are hand-duplicated** (`commands/types.rs` ↔ `types/app.ts`), not
  generated — a standing drift risk.
- **Sparse frontend tests** (~12 vitest files for ~191 TS files / ~40k LOC); no E2E.
- App layer less disciplined than the core: **71 commands over one mutex-locked
  `AppState`**, plus God-components (`App.tsx` ~1432 LOC, `TableEditor2D` ~1190).
- **Bus factor 1** (solo + AI authorship), alpha, no stable release, **GPL-2.0-only**
  (incompatible with OpenTune's recommended GPL-3 → its code cannot be lifted).

These weaknesses *are* OpenTune's differentiators: a committed firmware corpus,
a simulator-tested burn path, generated types, real `.mlg`, and multi-contributor
governance.

| Tool | What it is | Stack | Platforms | License | Live tuning? | State | Key weakness |
|---|---|---|---|---|---|---|---|
| [LibreTune](https://github.com/RallyPat/LibreTune) | **Near-identical to OpenTune** | Rust+Tauri+React | Win/Mac/Linux | GPL-2.0 | Yes | Active, alpha, 1 dev | alpha, bus-factor 1 |
| [TunerStudio](https://www.tunerstudio.com/) | Commercial incumbent (the target) | Java | Win/Mac/Linux | Proprietary, to ~$100 | Yes | Market leader | Java friction, dated UI, Mac issues, paywall |
| [HyperTuner Cloud](https://github.com/hyper-tuner/hypertuner-cloud) | Web tune/log share+view | React+Go | Web | MIT | No | Stalled since Feb 2024 | no serial/live |
| [VETuner](https://vetuner.co.uk/) | Browser live tuner (Web Serial) | JS | Web | Closed | Yes | Live | closed, no rusEFI |
| rusEFI / FOME Console | Dev/flash tool (TS is primary UI) | Java | Win/Mac/Linux | GPL | dev-level | Very active | not for everyday tuning |
| [UltraLog](https://github.com/ClassicMiniDIY/UltraLog) | Native log viewer | Rust (egui) | Win/Mac/Linux | AGPL-3.0 | No | Active | log analysis only |
| MegaTunix / EMStudio / msqdev | Older OSS attempts | C/Qt/Perl | various | GPL | legacy | **Dead** (2013–2016) | never reached TS parity |

**Commercial pro ECU software** (UX bar only — all **Windows-only**, native
Mac/Linux is uncontested): Haltech NSP (most modern), MoTeC M1/i2 (powerful,
dense), Holley (explicitly no Mac), ECUMaster (still 32-bit).

**Pattern to heed:** every prior OSS attempt except LibreTune died from
single-maintainer burnout *before reaching TunerStudio parity*. The moat is UX,
reliability, `.ini` breadth, and **sustained multi-contributor momentum** — not
secret protocol knowledge (everything is documented; see §3).

---

## 2. What TunerStudio users actually need

### Top pain points (ranked by frequency/strength of evidence)

1. **Connection drops mid-tune, won't auto-reconnect** — most pervasive reliability
   complaint. Speeduino counts the `secl` second-counter from boot, TS from
   handshake → reconnect loops; laptop power-saving drops USB 5V on screen-dim and
   often won't reconnect without a reboot.
   [speeduino t=604](https://speeduino.com/forum/viewtopic.php?t=604),
   [msextra t=67675](https://www.msextra.com/forums/viewtopic.php?t=67675),
   [t=38317](https://www.msextra.com/forums/viewtopic.php?t=38317)
2. **macOS + Java fragility** — wrong bundled JRE, hangs on "Initializing File
   Dialogs", firmware update "crashes as soon as I click next" on Catalina. Common
   workaround: ditch the Mac package, run the Linux `.jar`.
   [t=72627](https://www.msextra.com/forums/viewtopic.php?t=72627),
   [t=59690](https://www.msextra.com/forums/viewtopic.php?t=59690)
3. **Apple Silicon serial/Bluetooth instability** — native RS232 lib lacks ARM;
   WiFi "drops bits of data, log constantly pauses… annoying as hell"; Bluetooth
   "connects and then just disconnects".
   [t=78154](https://www.msextra.com/forums/viewtopic.php?t=78154)
4. **HiDPI/4K "basically unusable"** — tiny text/icons/tabs at 3200×1800; maxing
   fonts doesn't fix icons. Repeatedly patched in changelogs.
   [t=58557](https://www.msextra.com/forums/viewtopic.php?t=58557)
5. **Large-datalog lag in MegaLogViewer** — "10 seconds or more to zoom back in…
   slow and choppy regardless of log file"; dev reproduced at 100k+ records.
   [t=63293](https://www.msextra.com/forums/viewtopic.php?t=63293)
6. **VE Analyze (auto-tune) is non-deterministic** — different VE table every run on
   the same log; can "dump crazy amounts of fuel to an already rich condition".
   [miataturbo 97016](https://www.miataturbo.net/megasquirt-18/ve-analyze-showing-differnet-results-after-every-analysis-97016/)
7. **No tune diff / selective merge** — users resort to Git, BeyondCompare, Excel.
   "on the todo list" for TS for years.
   [t=67871](https://www.msextra.com/forums/viewtopic.php?t=67871),
   [Git-for-tuning writeup](https://burdickjp.gitlab.io/2015/09/16/usingGitForTuning.html)
8. **MLV can't compare two logs in scatter plots** — "load all the data into excel".
   [t=58708](https://www.msextra.com/forums/viewtopic.php?f=100&t=58708)
9. **Dashboard refresh is CPU-bound** — TS caps windowed mode to ~24 fps / 32% CPU;
   layered dashes stutter.
   [t=82691](https://www.msextra.com/forums/viewtopic.php?t=82691)
10. **Outdated documentation** — newest manuals ~2018; autotune guidance from 2005.
    [t=78718](https://www.msextra.com/forums/viewtopic.php?t=78718)

### Most-wanted missing features

- **Selective tune diff + merge** (single settings and table cells) — the
  most-repeated request.
- **Two-log compare in scatter plots** (currently Excel).
- **GUI math/filter-channel library** (TS uses raw string expressions; MoTeC i2 has
  derivatives/filters — people drop to Excel).
- **Native knock detection + logging** for Speeduino.
- **Cloud tune/log sharing** (community hand-built `speeduino/Tunes` on GitHub).
- **Capable mobile/tablet tuning** (Shadow Dash is dash-only, drops BT packets).

### What TunerStudio paywalls (and the friction it causes)

One-time licenses (not subscription): **TS Registered ~$70**, **Ultra ~$100**,
**MegaLogViewer ~$30 / HD ~$40 separate**.
[feature matrix](https://www.tunerstudio.com/index.php/products/tuner-studio/tsarticles/119-tunerstudio-30-feature-matrix)

- Behind **$70**: VE Analyze Live (auto-tune), high-speed logging (>15 Hz; Lite
  throttled to 15 rec/s), custom dashboards, WiFi/BT/FTDI comms, dark mode.
- Behind **$100** (Ultra): integrated log viewer, datalog playback, Tuning/Dyno
  views, Trim Table AutoTune.
- Friction: overlap (Ultra viewer ≠ MLV HD — serious users buy both),
  "free-beta-then-charge" resentment, 3-machine activation cap that clears only
  after 30–90 days.

---

## 3. Formats & protocol — openly documented (no reverse engineering needed)

Every layer has a first-party spec. **Caveat:** the EFI Analytics docs are marked
"proprietary" and the MS serial-protocol PDF states it "is not permitted for use
with other engine management systems" — so base the implementation on the **open
firmware source** (Speeduino/rusEFI), not on citing that PDF.

- **`.ini` ("INI dictionary")** — [EFI Analytics ECU Definition files.pdf](https://www.efianalytics.com/TunerStudio/docs/EFI%20Analytics%20ECU%20Definition%20files.pdf)
  (~111 pages; defines constants/offsets, tables, curves, gauges, menus, **and the
  serial command set**). De-facto truth is the firmware source:
  Speeduino `comms.cpp`/`comms_legacy.cpp`, rusEFI `tunerstudio.cpp` +
  `firmware/integration/ts_protocol.txt`.
- **MS serial protocol** — [Megasquirt_Serial_Protocol-2014-10-28.pdf](http://www.msextra.com/doc/pdf/Megasquirt_Serial_Protocol-2014-10-28.pdf).
  Framed packet = `length + command + payload + CRC32`, **no 0x00 start byte**
  (framing is by the length field); length excludes length+CRC bytes; framed
  responses carry a leading return-code byte before channel data.
  **Endianness split: MLG logs are Big-Endian, but Speeduino serial length/CRC are
  Little-Endian — handle separately.** Signature must byte-match the `.ini`
  `signature=` or TS rejects with "signatures do not match".
- **`.msq`** — self-describing XML (`<msq xmlns="http://www.msefi.com/:msq">` →
  `versionInfo`/`bibliography`/`page` → `constant`). **Stores only names+values,
  not memory layout — meaningless without its matching `.ini`.** Gate loading on
  signature + `fileFormat`.
- **`.mlg`** — [MLG_Binary_LogFormat_1.0.pdf](http://www.efianalytics.com/TunerStudio/docs/MLG_Binary_LogFormat_1.0.pdf)
  (magic `MLVLG\0`, 55-byte field records, `value = (raw + transform) * scale`,
  1-byte sum CRC). **Only v1 has a published byte-level spec**; v2/v3 are
  undocumented — reverse-engineer via `mlg-converter`.

### Reusable open-source building blocks (don't rebuild parsers)

- [`hyper-tuner/ini`](https://github.com/hyper-tuner/ini) — TS `.ini` parser (MIT, JS).
- [`adbancroft/TunerStudioIniParser`](https://github.com/adbancroft/TunerStudioIniParser) — `.ini` parser (Python).
- [`karniv00l/mlg-converter`](https://github.com/karniv00l/mlg-converter) — `.mlg` reader (JS).
- [`hyper-tuner/mlg-cli`](https://github.com/hyper-tuner/mlg-cli) — `.mlg` (Rust, abandoned at v0.1.0).
- [`askrejans/speeduino-serial-sim`](https://github.com/askrejans/speeduino-serial-sim) — full protocol simulator (test harness without hardware).
- [`noisymime/speeduino`](https://github.com/noisymime/speeduino) `comms.cpp` — reference protocol implementation.
- `speeduino/Tunes`, `DeeEmm/sparkduino` — real `.msq` examples.

---

## 4. Legal notes

- **Consuming firmware `.ini`** shipped by Speeduino/rusEFI is clean — those INIs
  are part of GPL firmware. A GPL app consuming them is fine.
- **EFI Analytics docs are "proprietary"** and the serial-protocol PDF is
  interoperability-restricted — implement from open firmware source, not the PDF.
- **Trademarks**: "TunerStudio" and "MegaSquirt" are marks — don't imply
  affiliation. Confirm "OpenTune" availability (other products use the name; hence
  "name TBD" in the README).
- **GPL + Apple notarization**: distributing outside the App Store with a Developer
  ID is fine under GPL-3. App Store would have friction (not a planned channel).
- **LibreTune is GPL-2.0**; ADR-0005 recommends GPL-3.0. **GPL-2-only and GPL-3 are
  incompatible** — this constrains any plan to reuse LibreTune code.

---

## 5. Where OpenTune can be genuinely better

The market structure favors a new entrant, and the differentiators map directly
onto the verified pain points above:

1. **Native Apple Silicon, no Java** → fixes pain points #2, #3 (the biggest
   structural advantage; no incumbent or pro tool is native Mac/Linux).
2. **Correct HiDPI/4K from day one** → #4 (web stack solves this for free).
3. **Bulletproof auto-reconnect** with `secl` resync → #1.
4. **Tune diff/merge + two-log scatter compare** → #7, #8 (TS has lacked these for
   years).
5. **Smooth dashboard** (canvas/WebGL, not capped at 24 fps) → #9.
6. **Everything TS paywalls, free** — auto-tune, high-speed logging, playback, dark
   mode, custom dashboards.
7. **Deterministic, auditable auto-tune** with visible data-filtering → #6.

The defensible differentiator is not any one feature but **reaching a trustworthy,
multi-contributor, stable write-to-ECU release** — the thing every dead predecessor
failed to do.

---

*Living document. Update as the landscape moves (LibreTune especially) and as
deeper investigations land.*
