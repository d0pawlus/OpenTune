# M5 implementation decisions

## Scope delivered

- `opentune-datalog` owns the in-memory log model and deterministic CSV and MLG
  v1 readers/writers. MLG uses the published 22-byte v1 header, 55-byte scalar
  field descriptors, big-endian values, record checksums, timestamp unwrapping,
  and marker blocks.
- `opentune-analysis` remains side-effect-free. `log_stats`, `detect_anomaly`,
  and `virtual_dyno` consume the same `SampleSet` seam as M4's `ve_analyze` and
  return explicit audit data.
- The owner task remains the only realtime consumer. Logging arms polling and
  receives every decoded acquisition before the UI emission gate. Disk and
  analysis work runs on the blocking pool.
- IPC transfers logs in columnar pages, capped at 100,000 records per request.
  This avoids one object allocation per point and lets the frontend load large
  logs incrementally.
- The frontend keeps two loaded snapshots for A/B comparison while the backend
  owns one active analysis/export log. uPlot is lazy-loaded and receives the
  column arrays directly.
- Playback reflects the selected row into the existing realtime store, so the
  dashboard and its gauges need no log-specific path.
- Math channels are configured through typed controls, not a free-text
  expression evaluator. M5 includes derivative, moving average, low-pass, and
  range-gating operations.

## Performance

The data path is pinned by tests using 100,000 records. Derived-channel
evaluation is linear and deterministic; chart rendering stays outside React
reconciliation and uPlot handles zoom/pan on canvas. No point reduction is
applied, preserving exact values for inspection and scatter comparison.

## Compatibility boundaries

- MLG support intentionally targets the published v1 scalar field types 0–7.
  MLG v2 and bit-field descriptors are rejected rather than guessed.
- CSV has no standard marker or units representation, so CSV export contains
  time and channel values only. MLG preserves markers and field metadata.
- A recording is buffered in memory and written atomically when stopped.
- MLG's 16-bit timestamp is unwrapped in record order; an unobservable gap
  longer than one complete timestamp cycle cannot be reconstructed.

## Verification

M5 is covered by Rust round-trip, malformed-input, deterministic-analysis,
owner/IPC, and simulator tests; frontend component, store, playback, math-channel,
and 100k-record tests; plus clippy, rustfmt, ESLint, Prettier, TypeScript, and the
production Vite build. Native GUI automation and real-ECU logging remain manual
hardware validation.
