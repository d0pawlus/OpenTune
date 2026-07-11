# Offline tune editing — design

- **Date:** 2026-07-06
- **Branch:** `offline-tune` (isolated worktree; parallel M4 work continues on `m4-table-editors`)
- **Status:** approved (design), pending spec review

## Goal

Let a user work on a tune **without an ECU connected**: pick their ECU's INI,
either start a new blank tune or open a saved `.msq`, edit it with the existing
table / curve / dialog editors, save it back to `.msq`, and later push it to the
ECU once connected.

User story (verbatim intent): *pick the INI for my ECU, load my saved tune if I
have one, work on it, save it, and load it to the ECU when connected.*

## Scope boundaries (approved)

1. **Push target = the simulator.** Offline "Write to ECU" reuses the existing
   `write_deltas` + `burn` path, which today is simulator-only
   (`session.rs` `SERIAL_UNSUPPORTED`). Real *serial* ECU write is a pre-existing,
   separate gap this feature does **not** close. The demo is: push an offline tune
   into the running simulator.
2. **Save format = `.msq`** (TunerStudio XML), used for both open *and* save — one
   interoperable format, no app-native format invented. Pulled forward from M6,
   trimmed (see [.msq scope](#msq-format-scope)).

## The core architectural change

Today the codebase conflates "having a tune to edit" with "having a live ECU
link": `Session.conn` is a mandatory `ActiveConnection` (`connection.rs:49`), and
every tune command routes through `with_session`, so editing errors `"not
connected"` without a live link. `TunePanel` also `reset()`s the tune store the
moment the link isn't alive (`TunePanel.tsx:90`).

**The change:** make the *connection* the optional part, not the session.

```rust
// connection.rs — before
struct Session { conn: ActiveConnection, def: Arc<Definition>, tune: Option<Tune>, snapshot: Option<Tune> }
// after
struct Session { conn: Option<ActiveConnection>, def: Arc<Definition>, tune: Option<Tune>, snapshot: Option<Tune> }
```

A session now always has `def` (from a picked INI) and a `tune`; it *may or may
not* have a live `conn`. Gating flips from "is there a session" → "does the
session have a `conn`". The three hardware-touching ops — live `write_deltas`,
`burn`, `start_realtime` — check `conn.is_some()` and return a
`"no connection"` error otherwise. Everything else (`get_values`, `set_value`,
`set_cells`, `undo`, `redo`, `diff`, `snapshot`) is **unchanged** and works
offline as-is.

**Rejected alternatives:** (B) a parallel set of offline-only commands duplicates
every editor write path and forces the frontend to branch on which command set to
call. (A2) a `Session::Offline`/`Online` enum duplicates `def`+`tune` across both
variants when they are always present. A1 models reality — tune always present,
connection optional — and gating stays localized to the three ops that already
build a protocol handle per-op (`session.rs:257`).

## Components: reuse vs build

| Piece | Disposition |
|---|---|
| INI → `Definition` | **reuse** `load_definition_from_path` (`connection.rs:113`) |
| Standalone editable tune | **reuse** `Tune::new(def)` (all pages zeroed) |
| Table / curve / dialog editors | **reuse untouched** (need only `definition`+`values` in `useTuneStore`) |
| `get/set/set_cells/undo/redo/diff/snapshot` | **reuse untouched** |
| `write_deltas` / `burn` | **reuse** for the push |
| `.msq` read/write | **build** in the empty `project` crate |
| `save_tune` / `open_tune` / `new_tune` commands | **build**, modeled on `layout.rs` file-persistence pattern |
| Native file pickers | **build** — add `tauri-plugin-dialog`; frontend picks path, Rust reads via `std::fs` |
| `conn: Option<…>` + gating | **build** — the core change in `connection.rs` / `owner.rs` / `session.rs` |
| Push whole tune to ECU | **build** — `write_tune_to_ecu` (all pages → burn) |
| Offline entry UI + `hasTune` store bit | **build** in the frontend |

## Command surface (new)

Modeled on `layout.rs` (plain owner commands, no live link required except the
push):

- `new_tune(ini_path: String) -> DefinitionDto` — parse INI, create session with
  `conn: None` and `Tune::new(def)`.
- `open_tune(ini_path: String, msq_path: String) -> DefinitionDto` — parse INI,
  create session with `conn: None`, `Tune::new(def)`, then apply the `.msq`
  values by constant name; **validate the `.msq` signature against the INI**
  before applying.
- `save_tune(path: String)` — serialize the current session's tune to `.msq`.
- `write_tune_to_ecu()` — **requires `conn`**; verify tune signature vs ECU
  identity, then write every page via `write_deltas` and `burn`.

INI/`.msq` path selection happens in the frontend via `tauri-plugin-dialog`; the
chosen path string is passed to these commands (mirrors how
`load_definition_from_path` already takes a path).

## Data flows

### Offline entry

```
[Work offline] → pick INI (dialog) → new_tune(ini) ─┐
                                                     ├→ session{conn:None, def, tune}
[Open tune…]   → pick INI + .msq    → open_tune(...) ┘   → DefinitionDto → store
```

Frontend then loads `definition` + `values` into `useTuneStore` exactly as the
online path does after a read. Editors light up. No connection involved.

### Editing & saving

Edits go through the unchanged `set_value` / `set_cells` (optimistic, with
rollback) against the in-memory `Tune`. `undo`/`redo`/`diff`/`snapshot` unchanged.
`save_tune(path)` reads the tune (`get` per constant) and writes `.msq`.

### Connect while an offline tune is loaded (the bug-prone flow)

The normal `connect` creates a fresh session and reads the tune from the ECU. With
an offline tune already loaded, doing that would **overwrite the user's edits**, so
`connect` branches:

```
connect(source):
  if session exists with conn==None and tune loaded:      # ATTACH mode
      open link → handshake identity
      verify def.signature == identity.signature
        └ mismatch → refuse attach, keep offline tune, surface error
      attach conn to the existing session          # do NOT load_tune
      → user may now click "Write to ECU"
  else:                                                     # FRESH mode (today)
      create session, load_tune() from ECU
```

The push is **never automatic on connect** — it is an explicit "Write to ECU"
action, so the ECU is never clobbered implicitly and the offline tune is never
implicitly discarded.

### Push to ECU

`write_tune_to_ecu()`: require `conn`; verify signature (guard #2 below); for each
page write the full page image via the existing `write_deltas` path, then `burn`.
Whole-tune write, not dirty-only — an offline tune has no read baseline.

### Disconnect while editing

Flip `TunePanel`'s reset condition: instead of `reset()` on `!linkAlive`, keep the
tune and drop to offline mode. The store gains a `hasTune` bit decoupled from
`linkAlive`; the store is only reset when there is **no conn and no tune**.

## Safety guards (must-haves, not polish)

1. **`.msq` ↔ INI on load** (`open_tune`): a `.msq` built for different firmware
   maps values onto the wrong page offsets. Compare the `.msq` signature to the
   INI signature; refuse on mismatch. If a `.msq` lacks a clean signature, fall
   back to a structural check (declared pages/sizes) and warn.
2. **tune ↔ ECU on push** (`connect` ATTACH, then `write_tune_to_ecu`): compare
   `def.comms.signature` to the connected ECU's handshake identity. On mismatch,
   ATTACH **refuses** — the connection is not attached to the offline session, the
   offline tune is kept, and the user is told the ECU's firmware does not match
   their tune's INI. Because ATTACH guarantees a matched signature before a `conn`
   ever reaches the session, `write_tune_to_ecu` re-checks the same equality as
   defense-in-depth and errors if it ever fails. No silent write, no auto-proceed;
   there is no "write anyway" override in the MVP (a deliberate follow-up).

## `.msq` format scope

`.msq` is self-describing XML (`<msq>` → `<page>` → `<constant name=… …>value</constant>`).

- **Round-trip only** the `<constant>` name→value pairs plus the signature.
- Values map through `Tune::get`/`Tune::set` by constant name, dispatched by
  `ConstantKind`:
  - **Scalar** → number string.
  - **Array** → space/newline-separated numbers → `Value::Array`.
  - **Bits** → option **label** string ↔ `Value::Enum(index)`, mapped through
    `ConstantKind::Bits{options}`. **This is the one round-trip corruption risk**
    and gets a dedicated label↔index test.
  - **Text** → string.
- **Skip** (ponytail-commented ceiling): `.msq` settings-groups, comments,
  CRC / bibliography / versionInfo metadata beyond the signature. Full `.msq`
  fidelity stays M6.

## Error handling

- File read/parse errors (bad INI, malformed `.msq`, unreadable path) surface as
  typed command errors → user-facing toast in the frontend; no partial session is
  created.
- Signature mismatches (load or push) are explicit, actionable errors — never
  silent, never auto-proceed.
- `write_tune_to_ecu` without `conn`, or the other live ops offline, return the
  `"no connection"` error the frontend already knows how to display.
- `.msq` values that fail `Tune::set` validation (out of INI `low`/`high`) are
  collected and reported; a load either applies fully or reports which constants
  were rejected (fail-loud, per-constant), leaving the tune in a defined state.

## Testing

- **Rust `project` crate:** `.msq` round-trip unit tests per `ConstantKind`, with
  the **bit-field label↔index** case as the anchor (the corruption risk). Load a
  fixture `.msq`, apply to a `Tune`, re-serialize, assert equality.
- **Rust session/owner:** offline session lifecycle — `new_tune`/`open_tune`
  create a `conn:None` session; `get/set/set_cells/undo/redo` work with no link;
  live ops error `"no connection"`; ATTACH-on-connect keeps the tune and does not
  `load_tune`; signature-mismatch refuses.
- **Frontend:** offline entry populates `useTuneStore`; editors render and edit
  with `linkAlive == false`; disconnect keeps the tune (no `reset`); store resets
  only with no conn and no tune.
- Signature-guard tests on both load and push.

## Files touched (anticipated)

- `src-tauri/crates/project/src/` — new `.msq` read/write module + tests.
- `src-tauri/crates/project/Cargo.toml` — XML dep (e.g. `quick-xml`), `model`,
  `ini` deps.
- `src-tauri/src/connection.rs` — `conn: Option<ActiveConnection>`.
- `src-tauri/src/session.rs` — gate live ops on `conn`; attach-mode helper;
  whole-tune write.
- `src-tauri/src/owner.rs` / `owner_ops.rs` — `new_tune`/`open_tune`/`save_tune`/
  `write_tune_to_ecu` handling; `connect` ATTACH branch.
- `src-tauri/src/commands.rs` / `tune_commands.rs` — new `#[tauri::command]`s.
- `src-tauri/src/lib.rs` — register commands; `tauri-plugin-dialog`.
- `src-tauri/Cargo.toml` + `package.json` — `tauri-plugin-dialog`.
- `src/stores/tune.ts` — `hasTune` decoupled from `linkAlive`.
- `src/components/dialogs/TunePanel.tsx` — reset only when no conn & no tune.
- `src/components/` — offline entry UI (pick INI, New blank / Open `.msq`, Save,
  Write to ECU), wired to the new commands + dialog plugin.
- `src/ipc/bindings.ts` — regenerated via `tauri-specta`.

## Out of scope / follow-ups

- Real serial-ECU write (`SERIAL_UNSUPPORTED`) — pre-existing, unchanged.
- Full `.msq` fidelity (settings groups, CRC, bibliography) — stays M6.
- Native project bundle (INI + tune + dashboard + settings) — M6 `project` crate
  goal beyond this slice.
- Recent-files / auto-locating the INI from a `.msq` — nice-to-have, not MVP.
