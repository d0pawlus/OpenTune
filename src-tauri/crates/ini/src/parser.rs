// SPDX-License-Identifier: GPL-3.0-or-later
//! INI comms-settings parser — M1 slice.
//!
//! Ported from `hyper-tuner/ini` (MIT, ADR-0006).

use crate::{CommsSettings, IniError, Result};

/// Parse the comms-settings slice from raw INI text.
pub fn parse_comms(_ini_text: &str) -> Result<CommsSettings> {
    Err(IniError::MissingKey("not yet implemented".to_owned()))
}
