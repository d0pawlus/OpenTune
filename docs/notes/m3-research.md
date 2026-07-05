# M3 — Real-time dashboard: research dossier

> **Research artifact** (2026-07-02), not a design doc. Input for the M3 implementation
> plan. Sources: this repo; `noisymime/speeduino@63fd68e9` (the SHA M2 pinned —
> `comms.cpp`, `comms_legacy.cpp`, `logger.h`, board headers, `reference/speeduino.ini`,
> all GPL-3); `hyper-tuner/ini@master` (MIT); `askrejans/speeduino-serial-sim@main` (MIT).

## A. INI `[OutputChannels]` and gauge sections (the data model)

Reference: `reference/speeduino.ini` @ 63fd68e9 (6 026 lines; signature
`"speeduino 202504-dev"`, iniSpecVersion 3.64). Sections in file order relevant to M3:
`[GaugeConfigurations]` (l. 5123), `[FrontPage]` (l. 5267), `[OutputChannels]` (l. 5346),
`[Datalog]` (l. 5642). None of the repo's trimmed fixtures
(`src-tauri/crates/ini/tests/fixtures/*.ini`, `src-tauri/resources/speeduino.sample.ini`)
contain any of these sections yet — M3 needs new fixtures.

### `[OutputChannels]` grammar

Header keys:

```ini
ochGetCommand    = "r\$tsCanId\x30%2o%2c"   ; note the backslash-escaped \$tsCanId
ochBlockSize     =  139
```

Three entry kinds (real file: **168** fixed `scalar`/`bits` entries, **78** computed):

1. **`scalar`** — same shape as `[Constants]` scalars, offset into the och block:
   `map = scalar, U16, 4, "kpa", 1.000, 0.000` (`name = scalar, TYPE, offset, units,
   scale, translate`; physical = raw·scale + translate). No min/max/digits.
2. **`bits`** — flag extraction from a byte already declared as a scalar:
   `running = bits, U08, 2, [0:0]`, multi-bit fields exist (`nSquirts = bits, U08, 84, [5:7]`).
   The backing byte is *also* declared (`engine = scalar, U08, 2, "bits", …`) — aliasing
   over the same offset is normal here (unlike the page-overflow rule in `[Constants]`).
3. **Computed channels** — `name = { expr }` with optional trailing `, "units"`:
   `coolant = { coolantRaw - 40 }`, `throttle = { tps }, "%"`,
   `dutyCycle = { rpm ? ( 100.0*pulseWidth/pulseLimit ) : 0 }`. Expressions reference
   other output channels, constants and PcVariables, chain onto other computed
   channels (`cycleTime → revolutionTime → rpm`), use ternaries (40 uses) and are
   wrapped in `#if CELSIUS` preprocessor blocks (already handled by `ini::preprocessor`).

Functions used inside `[OutputChannels]` expressions in the real file (all
`ExprError::UnsupportedFn` in today's evaluator, `expr_parser.rs:274-287`):
`bitStringValue(strList, idx)` — 3 uses, **only in `units`/`scale` fields** of scalar
entries (e.g. `idleLoad`, `fuelLoad`, `ignLoad`), not in channel value expressions;
`arrayValue(array.x, idx)` — 3 uses (`nFuelChannels` etc., feeds *dialog* conditions,
not gauges); `smoothBasic(chan, pct)` — 1 use (`loopsPerSecSmooth`). Special bare
variable: `timeNow` (`time = { timeNow }`) — wall-clock supplied by the app, not the ECU.
`table(...)` is **not used at all** in speeduino.ini's OutputChannels. So: plain
arithmetic/ternary evaluation over channel+constant symbols covers ~74 of 78 computed
channels; the function-using ones can degrade to diagnostics without hurting the
FrontPage gauges (none of the 8 default gauges nor the indicator list needs them,
except `idleLoadGauge`/`advance2Gauge` *units strings*, which can fall back to `""`).

### `[GaugeConfigurations]` grammar

```ini
gaugeCategory = "Main"
;Name       = Var,   Title,          Units, Lo, Hi, LoD, LoW, HiW, HiD, vd, ld
tachometer  = rpm,   "Engine Speed", "RPM",  0, {rpmhigh}, 300, 600, {rpmwarn}, {rpmdang}, 0, 0
```

12 positional fields after the channel var. Any of Units/Lo/Hi/LoD/LoW/HiW/HiD may be a
`{ expr }` (referencing PcVariables like `rpmhigh` or constants like `stoich`) or a
`bitStringValue(...)` for units. `vd`/`ld` = value/label decimal places. `#if CELSIUS`
gates some entries. `gaugeCategory` groups for menus.

### `[FrontPage]` grammar

```ini
gauge1 = tachometer        ; … gauge8 = gammaEnrichGauge   (2 rows × 4)
indicator = { running }, "Not Running", "Running", white, black, green, black
```

`gaugeN` slots reference GaugeConfigurations names. `indicator` lines: `{ expr }`,
off-label, on-label, off-bg, off-fg, on-bg, on-fg (named colors). ~40 indicators in the
real file; expressions are mostly bare bit-channels plus a few comparisons
(`{ (tps > tpsflood) && (rpm < crankRPM) }`, `{ sd_status & 1 }` — note **bitwise `&`
and `<<`** appear (`syncStatus = { halfSync + (sync << 1) }`), which the M2 evaluator
does not support → needs adding or degrading).

### `[Datalog]` — needed for M3?

`entry = channel, "Label", int|float, "%format" [, { condition }]` (labels may be
`{ stringValue(alias) }`). **Not needed for M3 gauges** — gauges bind via
FrontPage→GaugeConfigurations→OutputChannels. `[Datalog]` selects/labels the *logged*
subset → M5 (`datalog` crate). Recommend: parse in M3 only if trivially cheap, else defer.

### `ochGetCommand`/`ochBlockSize` semantics

`ochGetCommand` is the request template (expanded exactly like `pageReadCommand`);
`ochBlockSize` is the total byte length of one realtime frame = the `%2c` count for a
full read (139 for this INI; highest declared offset 138 `systemTempRaw`, U08).
Firmware cross-check: `logger.h:14` at the same SHA says `LOG_ENTRY_SIZE = 138` with a
comment "MUST match ochBlockSize" — a 1-byte skew in the dev tree. Consequence: trust
the INI's `ochBlockSize` for request length, tolerate short/padded responses.

## B. Wire protocol for realtime data (speeduino @ 63fd68e9)

### New (CRC-framed) protocol — the M3 default

Envelope both directions (`comms.cpp:476-558`, `sendBufferAndCrcNonBlocking`):

| bytes | meaning |
| --- | --- |
| 2 | payload length, **big-endian** u16 |
| N | payload |
| 4 | CRC32 of payload, **big-endian** (ISO-3309, same as `crc32_of` in `engine.rs`) |

`'r'` request payload (`processSerialCommand`, `comms.cpp:754-774`):

| offset | bytes | value |
| --- | --- | --- |
| 0 | 1 | `'r'` (0x72) |
| 1 | 1 | `$tsCanId` (0x00) — read but ignored by firmware |
| 2 | 1 | sub-command `0x30` = `SEND_OUTPUT_CHANNELS` (48) |
| 3–4 | 2 | offset, **little-endian** (`word(serialPayload[4], serialPayload[3])`) |
| 5–6 | 2 | length, **little-endian** |

Response payload: `[0] = 0x00 SERIAL_RC_OK`, then `length` bytes of the och block
starting at `offset` (`generateLiveValues`, `comms.cpp:359-374`). So a full-frame
response payload is `1 + 139` bytes inside the envelope. **Partial reads (offset/length
windows) are natively supported** — this is what TunerStudio's "high-speed logging"
uses to poll a subset faster. Return codes (`comms.cpp:49-57`): OK=0x00, BURN_OK=0x04,
TIMEOUT=0x80, CRC_ERR=0x82, UKWN_ERR=0x83, RANGE_ERR=0x84, BUSY_ERR=0x85 (TS retries).
Sub-command 0x0F over `'r'` returns the signature (alt path, ignore for M3).
First och request after connect resets `secl` to 0 (`generateLiveValues`,
`comms.cpp:361-365`) — reconnect's secl-resync must expect that.

### Legacy path

- Legacy `'r'` (`comms_legacy.cpp:315-341`): same 7 request bytes, raw (no envelope);
  response is **`length` raw bytes, no RC byte, no CRC** (`sendValues`,
  `comms_legacy.cpp:697-762` — echo of `r`+cmd happens only on *secondary* serial).
- Legacy `'A'` (`comms_legacy.cpp:89-90`): zero-argument, returns the full
  `LOG_ENTRY_SIZE` frame raw. The framed `'A'` also exists (`comms.cpp:604-606`).
  **Recommendation: skip `'A'` in M3** (record it): current Speeduino locks out legacy
  commands after the first CRC command (`comms.cpp:538`), our M1/M2 engine already
  speaks both `Plain` and `MsEnvelope10` via `ochGetCommand` expansion, and the INI's
  `ochGetCommand` is `'r'`-shaped. `'A'` support falls out for free anyway if
  `read_output_channels` just expands whatever `ochGetCommand` says (for an INI that
  declares `ochGetCommand = "A"`, the template has no `%2o/%2c` → full-frame read).

### Polling rates & limits

- TunerStudio's own guidance (tunerstudio.com "High Speed Logging"): baseline 115 200
  baud; MS2 ≈ 75 rec/s, MS3 ≥ 100 rec/s with partial reads; recommended UI data rate
  10–15 rec/s on slow machines (data-rate setting exists ~5–50).
- Physics: 115 200 baud ≈ 11 520 B/s; one full framed poll = 7+6 req + 140+6 rsp
  ≈ 159 B → ~72 Hz theoretical ceiling; USB latency makes 20–40 Hz realistic.
- `BLOCKING_FACTOR` (121 AVR / 251 Teensy; INI `blockingFactor`, `'f'` capability
  command) limits **page-read chunks**, not `'r'` responses — firmware serves 139-byte
  och frames on AVR fine (tx is non-blocking/resumable). No chunking needed for M3.
- ARCHITECTURE §9 target: acquisition at device rate, UI events coalesced to ~30 Hz.
  Plan: poll 25–30 Hz on serial, faster is pointless for gauges; simulator can poll faster.

## C. Existing OSS to port (ADR-0006)

- **`hyper-tuner/ini`** (MIT — re-confirmed via GitHub license API; JS/parsimmon,
  `src/ini.ts` 1 174 lines). Parses `[OutputChannels]` (`parseOutputChannels`,
  ini.ts:235-266 — reuses the same `parseConstAndVar` we already ported for
  `[Constants]` in M2, falling back to `{name, value}` for computed channels — it
  stores computed expressions **as opaque strings**) and `[Datalog]` (`parseDatalog`,
  ini.ts:195-233). It does **not** parse `[GaugeConfigurations]` or `[FrontPage]` at
  all (section switch ini.ts:146-190 ends at Datalog) — those two are write-fresh.
- **`askrejans/speeduino-serial-sim`** (MIT, C++, active). Yes — it animates realtime
  channels: `EngineSimulator` (EngineSimulator.h/.cpp) is a mode state machine
  (STARTUP → WARMUP_IDLE → IDLE → LIGHT_LOAD → ACCELERATION → HIGH_RPM → DECELERATION
  → WOT) with correlated physics (`simulateRPM/Thermal/MAP/Throttle/Fuel/Ignition/AFR`,
  VE curves, warm-up enrichment, sensor noise, 20 Hz update). `SpeeduinoProtocol.cpp`
  implements framed+legacy `'r'`/0x30 with offset/len windowing (lines 202-235,
  344-366) and confirms the byte layout in §B independently. **Port target:** the state
  machine + correlation structure into `opentune-simulator` (values must be *written
  into the INI-declared offsets*, not its hardcoded `EngineStatus` struct — that struct
  is a fixed 130-byte layout, ours must be definition-driven).
- **Gauges:** `canvas-gauges` (Mikhus, MIT, zero-dep radial+linear canvas gauges) is
  the only maintained MIT option; its React wrappers (`react-canvas-gauges`,
  `r-gauges`) are ~9 years stale. ARCHITECTURE §3 already records "2D table grids /
  gauges: HTML Canvas (custom)" as the stack decision — recommend hand-rolled canvas
  components (round/bar/digital/indicator), using canvas-gauges only as a *visual/API
  reference*. LibreTune's realtime store/simulator are **GPL-2.0-only — study only, no
  code** (ADR-0007).

## D. Existing OpenTune code to extend

Full detail with line refs sits in the subsections below; key contracts:

- **`crates/realtime`** is an empty M0 placeholder (`realtime/src/lib.rs` — one doc
  line, no deps in its `Cargo.toml`). Entirely greenfield.
- **`protocol`:** `Protocol` trait (lib.rs:112-154) has no realtime method.
  `expand_template`/`TemplateParams` (pages.rs:29-128) already supports **everything**
  `ochGetCommand` needs — `$tsCanId`, `\x30` hex escapes, `%2o` (LE), `%2c` (LE) — and
  a unit test (pages.rs:272-287) expands exactly `"r$tsCanId\x30%2o%2c"`. Missing: a
  `read_output_channels(offset, len)` caller. `read_secl` (engine.rs:227-271) is the
  precedent: it already sends `ochGetCommand`; in `MsEnvelope10` it reads the full
  frame and returns byte 0 (CRC **not** verified there — inline logic duplicating
  `envelope_read_bytes` (engine.rs:132-154, `pub(crate)`, CRC-verified) which the new
  method should reuse instead); in `Plain` it reads 1 byte and flushes the rest —
  unusable for M3 as-is. `CommsSettings` has no `tsCanId` field; `can_id` hardcoded 0.
- **`ini`:** `Definition` (definition.rs:33-54) has **no** output-channel /
  gauge / frontpage / datalog fields (grep-zero). `CommsSettings` (ini lib.rs:87-117)
  already carries `och_get_command` + `blocking_factor`, but no `ochBlockSize` —
  parser must start capturing it. Expression evaluator: arithmetic/comparison/boolean
  (eager `&&`/`||`), unary, bare symbols; **no function calls** (`UnsupportedFn`,
  expr_parser.rs:274-287) and **no bitwise `&`/`<<`** (used by a few indicators/
  computed channels, §A).
- **Concurrency (the §9 debt, m2-decisions.md:64-76):** today
  `SessionStore = Arc<Mutex<Option<Session>>>` (connection.rs:61); all commands sync;
  `protocol_for()` (session.rs:175-187) builds a **fresh `MsProtocol` per call and only
  for Sim** — serial page ops return `SERIAL_UNSUPPORTED` ("M3: persist MsProtocol in
  ConnectionManager"). `std::thread::sleep` runs while holding the mutex
  (pages.rs:239-244 delays; reconnect backoff reconnect.rs:122-124). Existing
  background-loop precedents: the `Heartbeat` thread (lib.rs:61-68, no session lock)
  and `simulate_link_drop_async` (connection.rs:196-242, `std::thread::spawn` that
  re-locks the store when done). **Migration sketch (owner-task + channel):** spawn one
  owner (Tokio task or dedicated thread — see open decisions) that *owns* `Session`
  (transport + persistent `MsProtocol` + `Tune`); replace `Mutex<Option<Session>>` with
  an `mpsc` command channel (`Connect/Disconnect/ReadPage/Write/Burn/StartRealtime/
  StopRealtime/...` + oneshot reply channels); Tauri commands become thin async senders;
  the realtime loop lives *inside* the owner (poll `'r'`, decode, push full-rate samples
  to future datalog, coalesce to ≤30 Hz `RealtimeFrameEvent`); reconnect integrates
  naturally (loop pauses in `Reconnecting`, resumes after resync — first och request
  after reboot resets secl, §B). This also unblocks serial live-writes (the persistent
  handle m2-decisions deferred).
- **Events/IPC pattern to mirror:** typed `tauri_specta::Event` structs in `events.rs`
  (`Heartbeat`, `TuneDirtyEvent`, `ConnectionStateEvent` with a `From<protocol type>`
  mirror impl); emit **after dropping the lock** (tune_commands.rs:63-71). New event +
  `collect_events![...]` in lib.rs:37-41 regenerates `src/ipc/bindings.ts`
  automatically. Commands follow `#[tauri::command] #[specta::specta]` +
  `State<SessionStore>` + `Result<_, String>` (commands.rs / tune_commands.rs list —
  `start_realtime`/`stop_realtime` slot in there).
- **Frontend patterns:** stores `src/stores/connection.ts` (reflect-only:
  `applyConnectionState`) and `src/stores/tune.ts` (optimistic writes + event-driven
  dirty). Realtime store should be reflect-only. Event listening is inline
  `useEffect` + `events.x.listen(cb)` calling `useStore.getState().applyY(payload)`
  (App.tsx:19-35) — deliberately not selector-subscribed. For 30 Hz gauge ticks,
  canvas gauges should read from the store imperatively (subscribe outside React or
  refs) so React reconciliation stays off the hot path (ARCHITECTURE §9); no such
  optimization exists yet to copy.
- **Simulator:** `respond_plain`/`respond_crc` (ecu.rs:194-283) handle only
  `Q/S/A/p/M/b`; **`'A'` returns a single byte (`secl`)** — no `'r'` arm, no channel
  data, no animation state (`Pipe` = cmd/rsp bufs + secl + `MemoryImage`). M3 adds: an
  `'r'`/0x30 arm with offset/len windowing over a definition-driven och block, and an
  animated engine model writing into that block (port of speeduino-serial-sim's state
  machine, §C).

## Port-vs-fresh ledger

| Surface | Decision | Source (license) | Notes |
| --- | --- | --- | --- |
| `[OutputChannels]` scalar/bits parsing | **Port** | hyper-tuner/ini MIT (`parseOutputChannels` + `parseConstAndVar`) | Extends the M2 port in `constants_fields.rs`; same field order |
| Computed-channel entries | **Port shape, fresh eval** | hyper-tuner stores `{expr}` as opaque string | Store expr string + lazy-eval with our `ini::expr` (per ADR-0006 note, evaluator stays fresh) |
| `[Datalog]` parsing | **Port (defer-able to M5)** | hyper-tuner/ini MIT (`parseDatalog`) | Not needed for gauges |
| `[GaugeConfigurations]`/`[FrontPage]` parsing | **Fresh** | — (hyper-tuner doesn't parse them) | Grammar documented in §A; record as write-fresh per ADR-0006 |
| `'r'` 0x30 request/decode (`read_output_channels`) | **Fresh, byte-confirmed** | Speeduino comms.cpp @63fd68e9 (GPL-3, truth source not port source) | Reuse `expand_template` + `envelope_read_bytes`; same pattern as M2 pages |
| Simulator engine model (animated channels) | **Port** | askrejans/speeduino-serial-sim MIT (`EngineSimulator`) | Re-target output into INI-declared offsets, not its fixed struct |
| Simulator `'r'` arm | **Port** | speeduino-serial-sim MIT (`SpeeduinoProtocol.cpp:202-235`) | Offset/len windowing incl. zero-pad past end |
| Canvas gauges (round/bar/digital/indicator) | **Fresh** | canvas-gauges MIT as visual reference only | ARCHITECTURE §3 already mandates custom canvas; record no-dep reason |
| Owner-task/channel concurrency | **Fresh** | ARCHITECTURE §9 is the spec | LibreTune's store is GPL-2-only — study only |
| Expression additions (bitwise `&`, `<<`) | **Fresh** | — | Small grammar extension to `expr_parser.rs`; keep `UnsupportedFn` degradation |

## Open decisions for the planner

1. **Tokio vs dedicated OS thread for the owner task.** §9 says Tokio, but transport
   (`serialport`) is blocking; a plain thread + `std::sync::mpsc` + blocking loop
   delivers the same invariant with less machinery (Tauri already hosts a Tokio
   runtime for async commands either way). Decide: full-Tokio (`spawn_blocking` for
   serial I/O) vs thread-owner + async command facade.
2. **Where channel decoding lives:** `realtime` crate (poll loop + decode + throttle,
   per ARCHITECTURE §5.5) vs decode in `model::Channels` (§5.4 names it). Suggested
   split: `ini` parses `OutputChannelDef`; `model` (or `realtime`) owns the
   scaled/computed view; `realtime` owns loop + throttling.
3. **Computed channels evaluation site:** backend (single source of truth, matches M2
   `eval_conditions` precedent) vs frontend (fewer IPC bytes). Backend recommended —
   emit already-scaled physical values per named channel.
4. **Event payload shape:** full frame map (`Vec<(name, f64)>` / `HashMap`) at ≤30 Hz
   vs only-gauge-bound channels. Full decoded frame is ~200 channels × f64 — fine at
   30 Hz; start simple (full frame), optimize later.
5. **Partial ('windowed') och reads:** support `%2o/%2c` windows now (they're free in
   the template) but poll full `ochBlockSize` in M3; per-gauge subsetting is an M5
   (high-speed logging) optimization.
6. **Gauge layout persistence:** ROADMAP says "editable layout saved with the
   project" — `project` crate is still a placeholder. Decide minimal persistence
   (JSON in app config dir?) vs pulling `project` work forward.
7. **`groupMenu` / table-aliasing blockers:** full real-INI ingestion still blocked by
   the M2-noted issues (aliased tables on page 5, comms keys in `[Constants]`,
   `groupMenu`). M3 gauges only need `[OutputChannels]`+gauge sections, which dodge
   those — but decide whether M3 is the milestone that finally loads the *unmodified*
   real speeduino.ini, or keeps trimmed fixtures.
8. **`start_realtime`/`stop_realtime` IPC contract** (per ARCHITECTURE §7): explicit
   commands vs auto-start on connect (protocol.md's connect sequence ends with "begin
   real-time polling" — auto-start with a pause command may fit better).

## Risks

- **Concurrency migration is the real M3 cost.** Rewiring `Mutex<Option<Session>>` +
  11 sync commands to owner-task/channel touches every command path; page ops and
  realtime interleave on one wire (poll must yield to writes/burns; TS does the same).
  Mitigate: land the channel refactor first with M2 behavior pinned by existing tests.
- **INI/firmware skew:** `ochBlockSize` 139 vs firmware `LOG_ENTRY_SIZE` 138 at the
  pinned SHA; tolerate short reads, never index past the received buffer.
- **Computed-channel expression coverage:** bitwise ops missing, `bitStringValue`/
  `arrayValue`/`smoothBasic`/`stringValue`/`timeNow` unsupported. Fail-open per channel
  (diagnostic + skip) so one bad expr can't kill the dashboard.
- **20–30 Hz through the WebView:** unthrottled store updates → re-render storms.
  Throttle in Rust (coalesce) *and* render gauges imperatively on canvas.
- **Serial realtime is unproven end-to-end:** M2 ran simulator-only for page ops; the
  persistent-`MsProtocol` seam (`SERIAL_UNSUPPORTED`, session.rs:32-33) must close in
  M3, and reconnect (+ secl reset on first och request) needs simulator drop tests.
- **License hygiene:** Speeduino sources are GPL-3 (compatible, but ports must carry
  attribution headers like M2's `pages.rs`); LibreTune remains GPL-2-only — no code.
- **Fixture size:** the full 473 KB speeduino.ini as a golden fixture will slow parser
  tests if parsed repeatedly; consider one full-file smoke test + trimmed section
  fixtures for the fast path.
