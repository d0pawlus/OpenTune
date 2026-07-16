// SPDX-License-Identifier: GPL-3.0-or-later
//! Definition-driven encoding of the simulator's realtime frame — the
//! **fresh** half of M3 Task 5 (see [`crate::engine`]'s port-note for the
//! ported half): the physics produce a [`ChannelValues`] snapshot; this
//! module writes it into the och block at whatever offsets/types the loaded
//! INI's `[OutputChannels]` declares. Raw encoding mirrors `opentune-model`'s
//! `codec::encode_raw` (`raw = round(physical / scale - translate)`, per
//! `ScalarType` + endianness), but clamps out-of-range values instead of
//! erroring — the sim must stay graceful, never panic its thread.

use opentune_ini::{Definition, Endianness, OutputChannelDef, ScalarType};

/// One tick's physical values, produced by [`crate::engine::SimEngine`].
/// Field semantics follow the Speeduino `[OutputChannels]` names they feed.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ChannelValues {
    pub(crate) secl: u8,
    pub(crate) rpm: i32,
    pub(crate) map_kpa: i32,
    pub(crate) baro_kpa: i32,
    /// Coolant temperature in °C (the raw sensor channel adds the
    /// firmware's +40 offset).
    pub(crate) coolant_c: i32,
    /// Intake air temperature in °C (same +40 raw convention).
    pub(crate) iat_c: i32,
    pub(crate) tps_percent: i32,
    /// Battery voltage in V × 10.
    pub(crate) battery_dv: i32,
    pub(crate) advance_deg: i32,
    pub(crate) afr_target: f64,
    /// M4 Task 9: the simulator's "measured" AFR — equal to `afr_target`
    /// when the loaded INI has no `[VeAnalyze]`/veTable binding, else
    /// drifts away from it wherever the bound `veTable` disagrees with the
    /// engine's hidden true-VE surface (see `crate::ve_model`).
    pub(crate) afr: f64,
    /// M4 Task 9: Speeduino's `egoCorrection` channel is 100-centered
    /// (100 = no trim); the sim never trims, so this is always 100.0.
    pub(crate) ego_correction: f64,
    pub(crate) running: bool,
    pub(crate) cranking: bool,
}

impl ChannelValues {
    /// Physical value for a named scalar channel, per the Speeduino
    /// `[OutputChannels]` naming. Unknown names are `None` — their block
    /// bytes are left untouched (zero).
    fn scalar(&self, name: &str) -> Option<f64> {
        Some(match name {
            "secl" => f64::from(self.secl),
            "rpm" => f64::from(self.rpm),
            "map" => f64::from(self.map_kpa),
            "baro" => f64::from(self.baro_kpa),
            // Raw temperature sensors carry the firmware's +40 °C offset
            // (the INI pairs them with `{ raw - 40 }` computed channels).
            "coolantRaw" => f64::from(self.coolant_c + 40),
            "iatRaw" => f64::from(self.iat_c + 40),
            "coolant" => f64::from(self.coolant_c),
            "iat" => f64::from(self.iat_c),
            "tps" | "throttle" => f64::from(self.tps_percent),
            "batteryVoltage" => f64::from(self.battery_dv) / 10.0,
            "advance" => f64::from(self.advance_deg),
            "afrTarget" => self.afr_target,
            "afr" => self.afr,
            "egoCorrection" => self.ego_correction,
            // Speeduino `engine` bitfield: BIT_ENGINE_RUN=0, BIT_ENGINE_CRANK=1.
            "engine" => f64::from(u8::from(self.running) | (u8::from(self.cranking) << 1)),
            _ => return None,
        })
    }

    /// Value for a named bits channel (flag view over an existing byte).
    fn bits(&self, name: &str) -> Option<u64> {
        Some(match name {
            "running" => u64::from(self.running),
            "crank" | "cranking" => u64::from(self.cranking),
            _ => return None,
        })
    }
}

/// Encode every recognized channel into `block` at its INI-declared
/// offset/type. Pure serialization of an already-sampled snapshot — safe to
/// re-run without advancing the model.
pub(crate) fn encode_channels(
    channels: &[OutputChannelDef],
    endian: Endianness,
    values: &ChannelValues,
    block: &mut [u8],
) {
    for channel in channels {
        match channel {
            OutputChannelDef::Scalar {
                name,
                kind,
                offset,
                scale,
                translate,
                ..
            } => {
                if let Some(physical) = values.scalar(name) {
                    // Inverse of TunerStudio's physical = (raw + translate) * scale.
                    let raw = if *scale != 0.0 {
                        (physical / scale - translate).round()
                    } else {
                        0.0
                    };
                    write_scalar(block, *offset, *kind, endian, raw);
                }
            }
            OutputChannelDef::Bits {
                name,
                storage,
                offset,
                bit_lo,
                bit_hi,
            } => {
                if let Some(value) = values.bits(name) {
                    write_bits(block, *offset, *storage, *bit_lo, *bit_hi, endian, value);
                }
            }
            // Computed channels are client-side expressions over the scalar
            // bytes — nothing of theirs exists on the wire.
            OutputChannelDef::Computed { .. } => {}
        }
    }
}

/// Block size: `ochBlockSize`, else (documented fallback when the INI omits
/// it) the smallest block covering every declared scalar/bits channel.
pub(crate) fn block_size(def: &Definition) -> usize {
    let declared = def.comms.och_block_size as usize;
    if declared > 0 {
        return declared;
    }
    def.output_channels
        .iter()
        .filter_map(|channel| match channel {
            OutputChannelDef::Scalar { offset, kind, .. } => offset.checked_add(width(*kind)),
            OutputChannelDef::Bits {
                offset, storage, ..
            } => offset.checked_add(width(*storage)),
            OutputChannelDef::Computed { .. } => None,
        })
        .max()
        .unwrap_or(0)
}

/// Byte width of a scalar storage type (mirrors opentune-model's codec).
/// `pub(crate)`: also reused by [`crate::ve_model`]'s array decode (the
/// inverse direction — raw bytes back to physical — needs the same widths).
pub(crate) fn width(kind: ScalarType) -> usize {
    match kind {
        ScalarType::U08 | ScalarType::S08 => 1,
        ScalarType::U16 | ScalarType::S16 => 2,
        ScalarType::U32 | ScalarType::S32 | ScalarType::F32 => 4,
    }
}

/// Write one raw scalar at `offset`, clamped into its storage range. An
/// offset past the block end is skipped, never a panic.
fn write_scalar(block: &mut [u8], offset: usize, kind: ScalarType, endian: Endianness, raw: f64) {
    let raw = if raw.is_finite() { raw } else { 0.0 };
    let bytes: Vec<u8> = match kind {
        ScalarType::U08 => vec![raw.clamp(0.0, f64::from(u8::MAX)) as u8],
        ScalarType::S08 => {
            vec![(raw.clamp(f64::from(i8::MIN), f64::from(i8::MAX)) as i8) as u8]
        }
        ScalarType::U16 => endian_bytes_u16(raw.clamp(0.0, f64::from(u16::MAX)) as u16, endian),
        ScalarType::S16 => endian_bytes_u16(
            raw.clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16 as u16,
            endian,
        ),
        ScalarType::U32 => endian_bytes_u32(raw.clamp(0.0, f64::from(u32::MAX)) as u32, endian),
        ScalarType::S32 => endian_bytes_u32(
            raw.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32 as u32,
            endian,
        ),
        ScalarType::F32 => match endian {
            Endianness::Little => (raw as f32).to_le_bytes().to_vec(),
            Endianness::Big => (raw as f32).to_be_bytes().to_vec(),
        },
    };
    let Some(end) = offset.checked_add(bytes.len()) else {
        return;
    };
    if let Some(dst) = block.get_mut(offset..end) {
        dst.copy_from_slice(&bytes);
    }
}

fn endian_bytes_u16(value: u16, endian: Endianness) -> Vec<u8> {
    match endian {
        Endianness::Little => value.to_le_bytes().to_vec(),
        Endianness::Big => value.to_be_bytes().to_vec(),
    }
}

fn endian_bytes_u32(value: u32, endian: Endianness) -> Vec<u8> {
    match endian {
        Endianness::Little => value.to_le_bytes().to_vec(),
        Endianness::Big => value.to_be_bytes().to_vec(),
    }
}

/// Read-modify-write a bit range within its storage bytes, preserving the
/// neighboring bits. Malformed ranges or out-of-block offsets are skipped.
fn write_bits(
    block: &mut [u8],
    offset: usize,
    storage: ScalarType,
    bit_lo: u8,
    bit_hi: u8,
    endian: Endianness,
    value: u64,
) {
    let w = width(storage);
    let storage_bits = (w * 8) as u8;
    if bit_lo > bit_hi || bit_hi >= storage_bits {
        return;
    }
    let Some(end) = offset.checked_add(w) else {
        return;
    };
    let Some(region) = block.get_mut(offset..end) else {
        return;
    };
    let mut pattern: u64 = 0;
    for (i, &byte) in region.iter().enumerate() {
        pattern |= u64::from(byte) << byte_shift(endian, w, i);
    }
    let mask = (1u64 << (bit_hi - bit_lo + 1)) - 1;
    pattern = (pattern & !(mask << bit_lo)) | ((value & mask) << bit_lo);
    for (i, byte) in region.iter_mut().enumerate() {
        *byte = (pattern >> byte_shift(endian, w, i)) as u8;
    }
}

/// Bit shift of byte `i` within a `w`-byte storage pattern.
fn byte_shift(endian: Endianness, w: usize, i: usize) -> usize {
    match endian {
        Endianness::Little => 8 * i,
        Endianness::Big => 8 * (w - 1 - i),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentune_ini::parse_definition;

    #[test]
    fn write_scalar_clamps_to_storage_range_and_skips_out_of_block_offsets() {
        let mut block = vec![0u8; 3];
        write_scalar(&mut block, 0, ScalarType::U08, Endianness::Little, 300.0);
        assert_eq!(block[0], 255, "over-range U08 must clamp, not wrap");
        write_scalar(&mut block, 1, ScalarType::U16, Endianness::Big, 4_660.0); // 0x1234
        assert_eq!(&block[1..3], &[0x12, 0x34], "big-endian U16 layout");
        // Offset past the block: skipped, never a panic.
        write_scalar(&mut block, 2, ScalarType::U16, Endianness::Little, 1.0);
        assert_eq!(block, vec![255, 0x12, 0x34]);
        write_scalar(
            &mut block,
            usize::MAX,
            ScalarType::U16,
            Endianness::Little,
            1.0,
        );
        assert_eq!(block, vec![255, 0x12, 0x34]);
    }

    #[test]
    fn write_bits_preserves_neighboring_bits() {
        let mut block = vec![0b1010_0100u8];
        write_bits(&mut block, 0, ScalarType::U08, 0, 0, Endianness::Little, 1);
        assert_eq!(block[0], 0b1010_0101);
        // Malformed range (hi past the storage): skipped.
        write_bits(&mut block, 0, ScalarType::U08, 4, 9, Endianness::Little, 1);
        assert_eq!(block[0], 0b1010_0101);
        write_bits(
            &mut block,
            usize::MAX,
            ScalarType::U16,
            0,
            0,
            Endianness::Little,
            1,
        );
        assert_eq!(block[0], 0b1010_0101);
    }

    #[test]
    fn block_size_falls_back_to_max_channel_extent_when_ini_omits_it() {
        let mut def = parse_definition(include_str!(
            "../../ini/tests/fixtures/speeduino-output-channels.ini"
        ))
        .expect("fixture parses");
        assert_eq!(block_size(&def), 16, "declared ochBlockSize wins");
        def.comms.och_block_size = 0;
        // Fixture channels end at tps (U08 @7) → smallest covering block = 8.
        assert_eq!(block_size(&def), 8);

        let scalar = def
            .output_channels
            .iter_mut()
            .find_map(|channel| match channel {
                OutputChannelDef::Scalar { offset, .. } => Some(offset),
                _ => None,
            })
            .expect("fixture has a scalar channel");
        *scalar = usize::MAX;
        for channel in &mut def.output_channels {
            match channel {
                OutputChannelDef::Scalar { offset, .. } | OutputChannelDef::Bits { offset, .. } => {
                    *offset = usize::MAX
                }
                OutputChannelDef::Computed { .. } => {}
            }
        }
        assert_eq!(
            block_size(&def),
            0,
            "overflowing channel extents are ignored rather than panicking"
        );
    }
}
