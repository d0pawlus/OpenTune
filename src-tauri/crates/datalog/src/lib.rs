// SPDX-License-Identifier: GPL-3.0-or-later
//! Deterministic datalog model plus CSV and published MLG v1 interoperability.
//!
//! MLG support follows EFI Analytics' public “Binary MLG Logging file format
//! specification 1.0”.  In particular, this crate does not invent extensions:
//! v2 files and unknown block/field types are rejected.

mod csv;
mod error;
mod mlg;
mod model;

pub use csv::{read_csv, write_csv};
pub use error::{DatalogError, Result};
pub use mlg::{read_mlg_v1, write_mlg_v1};
pub use model::{Field, FieldType, Log, LogEntry, Marker, Record};

use std::io::{Read, Write};

/// Supported on-disk formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Csv,
    MlgV1,
}

/// Read a log from an already-open stream.
pub fn read_log<R: Read>(reader: R, format: LogFormat) -> Result<Log> {
    match format {
        LogFormat::Csv => read_csv(reader),
        LogFormat::MlgV1 => read_mlg_v1(reader),
    }
}

/// Write a log to an already-open stream.
pub fn write_log<W: Write>(log: &Log, writer: W, format: LogFormat) -> Result<()> {
    match format {
        LogFormat::Csv => write_csv(log, writer),
        LogFormat::MlgV1 => write_mlg_v1(log, writer),
    }
}
