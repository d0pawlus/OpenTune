// SPDX-License-Identifier: GPL-3.0-or-later
//! `opentune-model` — the M2 in-memory tune model.
//!
//! [`Tune`] is an editable, in-memory snapshot of an ECU's page bytes, built
//! from an [`opentune_ini::Definition`]. This crate owns decoding/encoding
//! constant values, dirty tracking, and undo/redo; the `ini` crate owns
//! describing what the bytes mean.

mod edit;
mod tune;
mod value;

pub use tune::{ModelError, Tune};
pub use value::Value;
