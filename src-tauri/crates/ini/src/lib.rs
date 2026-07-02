// SPDX-License-Identifier: GPL-3.0-or-later
//! `opentune-ini` — parse a firmware INI definition.
//!
//! M1 needs only the *connect & identify* slice of the INI: the firmware
//! **signature** and the **communication settings**. Full constants/pages/UI
//! parsing is M2 (see [ini-format.md](../../../docs/ini-format.md)).
//!
//! Per [ADR-0006](../../../docs/adr/0006-reuse-existing-parsers.md), the parser
//! is **ported** from a proven open reference
//! ([`hyper-tuner/ini`](https://github.com/hyper-tuner/ini), MIT — compatible
//! with this GPL-3 project) rather than re-derived from the spec.
//!
//! # M1 contract (shared seam)
//!
//! [`CommsSettings`] is the fixed struct the `protocol` and `transport` agents
//! read from. Field names mirror the real INI keywords (verified against the
//! current Speeduino `speeduino.ini`):
//! `signature`, `queryCommand`, `versionInfo`, `pageActivationDelay`,
//! `blockReadTimeout`, `interWriteDelay`, `blockingFactor`, `endianness`,
//! `messageEnvelopeFormat`, `pageReadCommand`, `pageValueWrite`, `burnCommand`,
//! `ochGetCommand`. The parsing agent fills [`parse_comms`]; the *shape* is
//! frozen here so downstream work can begin in parallel.

mod constants;
mod constants_fields;
mod constants_parser;
mod definition;
mod parser;
mod preprocessor;
mod ui;

pub use constants::{ConstantDef, ConstantKind, Number, ScalarType, Shape};
pub use definition::{parse_definition, Definition, PageDef};
pub use parser::parse_comms;
pub use preprocessor::preprocess;
pub use ui::{
    CurveDef, Diagnostic, DialogDef, DialogField, FieldKind, MenuDef, MenuItem, TableDef,
};

/// Byte/field order of multi-byte values, taken from the INI `endianness` key.
///
/// Derives `serde::Serialize` + `specta::Type` because it is reachable from
/// [`Definition`] via [`CommsSettings`], which the frontend consumes over IPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, specta::Type)]
pub enum Endianness {
    /// `endianness = little` (Speeduino default).
    #[default]
    Little,
    /// `endianness = big` (some MS-family firmwares).
    Big,
}

/// How requests/responses are framed on the wire, from `messageEnvelopeFormat`.
///
/// Newer firmware wraps payloads with a length prefix + CRC32 (the "CRC
/// protocol"); legacy firmware sends them raw. The protocol engine selects its
/// framing from this (see [protocol.md](../../../docs/protocol.md)).
///
/// Derives `serde::Serialize` + `specta::Type` because it is reachable from
/// [`Definition`] via [`CommsSettings`], which the frontend consumes over IPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
pub enum EnvelopeFormat {
    /// Legacy unframed bytes (no length prefix, no CRC).
    Plain,
    /// `msEnvelope_1.0` — length-prefixed payload with a trailing CRC32.
    MsEnvelope10,
}

/// The communication slice of a parsed INI — everything `protocol`/`transport`
/// need to open a link and identify the ECU. Nothing here describes memory
/// layout; that is the M2 `Definition`.
///
/// Commands are kept as the raw INI template strings (e.g. `"p%2i%2o%2c"`); the
/// `protocol` crate owns expanding the `%2i`/`%2o`/`%2c`/`%v` placeholders. The
/// `ini` crate does **not** interpret them — it only extracts them faithfully.
///
/// Derives `serde::Serialize` + `specta::Type` because it is reachable from
/// [`Definition`], which the frontend consumes over IPC.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct CommsSettings {
    /// `signature` — the exact identity string the ECU must report
    /// (e.g. `"speeduino 202504-dev"`). Matched on connect.
    pub signature: String,
    /// `queryCommand` — the signature/query command character (e.g. `"Q"`).
    pub query_command: String,
    /// `versionInfo` — the human-readable version query command (e.g. `"S"`).
    pub version_info: String,
    /// `ochGetCommand` — the output-channel (realtime) get command. Used by
    /// `realtime` in M3 but parsed in M1 so the contract is complete.
    pub och_get_command: String,
    /// `pageReadCommand` — template for reading a config page (e.g. `"p%2i%2o%2c"`).
    pub page_read_command: String,
    /// `pageValueWrite` — template for a live write (e.g. `"M%2i%2o%2c%v"`).
    pub page_value_write: String,
    /// `burnCommand` — template to persist RAM→flash (e.g. `"b%2i"`).
    pub burn_command: String,
    /// `blockingFactor` — max payload bytes per block (e.g. 121 or 251).
    pub blocking_factor: u32,
    /// `pageActivationDelay` — ms to wait after selecting a page (e.g. 10).
    pub page_activation_delay_ms: u32,
    /// `blockReadTimeout` — ms to wait for a block response (e.g. 2000). Seeds
    /// the transport [`read_timeout`](../opentune_transport/struct.SerialConfig.html).
    pub block_read_timeout_ms: u32,
    /// `interWriteDelay` — ms between consecutive writes (e.g. 10).
    pub inter_write_delay_ms: u32,
    /// `endianness` — multi-byte field order.
    pub endianness: Endianness,
    /// `messageEnvelopeFormat` — wire framing (plain vs CRC-wrapped).
    pub envelope: EnvelopeFormat,
}

/// Errors the M1 INI slice can produce.
#[derive(Debug, thiserror::Error)]
pub enum IniError {
    /// A required comms keyword was absent from the file.
    #[error("missing required INI key: `{0}`")]
    MissingKey(String),
    /// A keyword was present but its value did not parse (bad number, unknown
    /// enum, …).
    #[error("invalid value for `{key}`: {detail}")]
    InvalidValue { key: String, detail: String },
}

/// Result alias for INI parsing.
pub type Result<T> = std::result::Result<T, IniError>;
