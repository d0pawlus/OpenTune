// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::{Read, Write};

use crate::{DatalogError, Field, FieldType, Log, LogEntry, Marker, Record, Result};

const MAGIC: &[u8; 6] = b"MLVLG\0";
const VERSION: u16 = 1;
const HEADER_LEN: usize = 22;
const FIELD_DESCRIPTOR_LEN: usize = 55;
const MARKER_LEN: usize = 54;

pub fn write_mlg_v1<W: Write>(log: &Log, mut writer: W) -> Result<()> {
    validate_fields(&log.fields)?;
    let payload_len: usize = log
        .fields
        .iter()
        .map(|field| field.field_type.width())
        .sum();
    let payload_len_u16 = u16::try_from(payload_len)
        .map_err(|_| DatalogError::InvalidHeader("record payload exceeds 65535 bytes".into()))?;
    let descriptor_bytes = log
        .fields
        .len()
        .checked_mul(FIELD_DESCRIPTOR_LEN)
        .ok_or_else(|| DatalogError::InvalidHeader("field descriptor size overflow".into()))?;
    let data_start = HEADER_LEN
        .checked_add(descriptor_bytes)
        .ok_or_else(|| DatalogError::InvalidHeader("data offset overflow".into()))?;
    let data_start_u32 = u32::try_from(data_start)
        .map_err(|_| DatalogError::InvalidHeader("data offset exceeds u32".into()))?;
    let field_count = u16::try_from(log.fields.len())
        .map_err(|_| DatalogError::InvalidHeader("more than 65535 fields".into()))?;

    writer.write_all(MAGIC)?;
    writer.write_all(&VERSION.to_be_bytes())?;
    writer.write_all(&log.started_unix.to_be_bytes())?;
    writer.write_all(&0u16.to_be_bytes())?; // optional Info Data absent
    writer.write_all(&data_start_u32.to_be_bytes())?;
    writer.write_all(&payload_len_u16.to_be_bytes())?;
    writer.write_all(&field_count.to_be_bytes())?;

    for field in &log.fields {
        let mut descriptor = [0u8; FIELD_DESCRIPTOR_LEN];
        descriptor[0] = field.field_type.code();
        put_c_string(&mut descriptor[1..35], &field.name);
        put_c_string(&mut descriptor[35..45], &field.units);
        descriptor[45] = field.display_style;
        descriptor[46..50].copy_from_slice(&field.scale.to_bits().to_be_bytes());
        descriptor[50..54].copy_from_slice(&field.transform.to_bits().to_be_bytes());
        descriptor[54] = field.digits;
        writer.write_all(&descriptor)?;
    }

    for (index, entry) in log.entries.iter().enumerate() {
        match entry {
            LogEntry::Record(record) => {
                if record.values.len() != log.fields.len() {
                    return Err(DatalogError::InvalidRecord {
                        index,
                        reason: format!(
                            "{} values for {} fields",
                            record.values.len(),
                            log.fields.len()
                        ),
                    });
                }
                writer.write_all(&[0, record.counter])?;
                writer.write_all(&(record.timestamp_10us as u16).to_be_bytes())?;
                let mut payload = Vec::with_capacity(payload_len);
                for (field, value) in log.fields.iter().zip(&record.values) {
                    write_raw(&mut payload, field, *value)?;
                }
                writer.write_all(&payload)?;
                writer.write_all(&[payload
                    .iter()
                    .fold(0u8, |sum, byte| sum.wrapping_add(*byte))])?;
            }
            LogEntry::Marker(marker) => write_marker(&mut writer, marker, index)?,
        }
    }
    Ok(())
}

pub fn read_mlg_v1<R: Read>(mut reader: R) -> Result<Log> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    if bytes.len() < HEADER_LEN {
        return Err(DatalogError::Truncated("header"));
    }
    let magic: [u8; 6] = bytes[0..6].try_into().expect("fixed slice");
    if &magic != MAGIC {
        return Err(DatalogError::InvalidMagic(magic));
    }
    let version = be_u16(&bytes[6..8]);
    if version != VERSION {
        return Err(DatalogError::UnsupportedVersion(version));
    }
    let started_unix = be_u32(&bytes[8..12]);
    let info_start = be_u16(&bytes[12..14]) as usize;
    let data_start = be_u32(&bytes[14..18]) as usize;
    let record_len = be_u16(&bytes[18..20]) as usize;
    let field_count = be_u16(&bytes[20..22]) as usize;
    let descriptors_end = HEADER_LEN
        .checked_add(
            field_count
                .checked_mul(FIELD_DESCRIPTOR_LEN)
                .ok_or_else(|| {
                    DatalogError::InvalidHeader("field descriptor size overflow".into())
                })?,
        )
        .ok_or_else(|| DatalogError::InvalidHeader("data offset overflow".into()))?;
    if data_start < descriptors_end {
        return Err(DatalogError::InvalidHeader(format!(
            "data starts at {data_start}, before descriptors end at {descriptors_end}"
        )));
    }
    if info_start != 0 && !(descriptors_end..data_start).contains(&info_start) {
        return Err(DatalogError::InvalidHeader(format!(
            "Info Data starts at invalid offset {info_start}"
        )));
    }
    if data_start > bytes.len() {
        return Err(DatalogError::Truncated("field descriptors"));
    }

    let mut fields = Vec::with_capacity(field_count);
    for index in 0..field_count {
        let offset = HEADER_LEN + index * FIELD_DESCRIPTOR_LEN;
        let descriptor = &bytes[offset..offset + FIELD_DESCRIPTOR_LEN];
        let field_type = FieldType::from_code(descriptor[0])
            .ok_or(DatalogError::UnsupportedFieldType(descriptor[0]))?;
        let scale = f32::from_bits(be_u32(&descriptor[46..50]));
        let transform = f32::from_bits(be_u32(&descriptor[50..54]));
        if !scale.is_finite() || scale == 0.0 || !transform.is_finite() {
            return Err(DatalogError::InvalidField {
                index,
                reason: "scale must be finite/non-zero and transform finite".into(),
            });
        }
        fields.push(Field {
            name: read_c_string(&descriptor[1..35], index, "name")?,
            units: read_c_string(&descriptor[35..45], index, "units")?,
            field_type,
            display_style: descriptor[45],
            scale,
            transform,
            digits: descriptor[54],
        });
    }
    let computed_record_len: usize = fields.iter().map(|field| field.field_type.width()).sum();
    if record_len != computed_record_len {
        return Err(DatalogError::InvalidHeader(format!(
            "record length is {record_len}, field descriptors require {computed_record_len}"
        )));
    }

    let mut log = Log {
        fields,
        started_unix,
        entries: Vec::new(),
    };
    let mut offset = data_start;
    let mut last_timestamp = None;
    let mut timestamp_wrap = 0u64;
    while offset < bytes.len() {
        let kind = bytes[offset];
        let index = log.entries.len();
        match kind {
            0 => {
                let end = offset
                    .checked_add(5 + record_len)
                    .ok_or(DatalogError::Truncated("data record"))?;
                if end > bytes.len() {
                    return Err(DatalogError::Truncated("data record"));
                }
                let counter = bytes[offset + 1];
                let timestamp_10us = unwrap_timestamp(
                    be_u16(&bytes[offset + 2..offset + 4]),
                    &mut last_timestamp,
                    &mut timestamp_wrap,
                );
                let mut cursor = offset + 4;
                let payload_end = cursor + record_len;
                let expected_crc = bytes[payload_end];
                let actual_crc = bytes[cursor..payload_end]
                    .iter()
                    .fold(0u8, |sum, byte| sum.wrapping_add(*byte));
                if actual_crc != expected_crc {
                    return Err(DatalogError::InvalidRecord {
                        index,
                        reason: format!(
                            "CRC mismatch: stored {expected_crc:#04x}, computed {actual_crc:#04x}"
                        ),
                    });
                }
                let mut values = Vec::with_capacity(log.fields.len());
                for field in &log.fields {
                    let width = field.field_type.width();
                    let raw = read_raw(field.field_type, &bytes[cursor..cursor + width]);
                    values.push((raw + f64::from(field.transform)) * f64::from(field.scale));
                    cursor += width;
                }
                log.entries.push(LogEntry::Record(Record {
                    counter,
                    timestamp_10us,
                    values,
                }));
                offset = end;
            }
            1 => {
                let end = offset
                    .checked_add(MARKER_LEN)
                    .ok_or(DatalogError::Truncated("marker"))?;
                if end > bytes.len() {
                    return Err(DatalogError::Truncated("marker"));
                }
                log.entries.push(LogEntry::Marker(Marker {
                    counter: bytes[offset + 1],
                    timestamp_10us: unwrap_timestamp(
                        be_u16(&bytes[offset + 2..offset + 4]),
                        &mut last_timestamp,
                        &mut timestamp_wrap,
                    ),
                    text: read_marker_text(&bytes[offset + 4..end], index)?,
                }));
                offset = end;
            }
            other => return Err(DatalogError::UnsupportedBlockType(other)),
        }
    }
    Ok(log)
}

fn validate_fields(fields: &[Field]) -> Result<()> {
    for (index, field) in fields.iter().enumerate() {
        if !field.name.is_ascii() || field.name.len() > 33 {
            return Err(DatalogError::InvalidField {
                index,
                reason: "name must be ASCII and at most 33 bytes".into(),
            });
        }
        if !field.units.is_ascii() || field.units.len() > 9 {
            return Err(DatalogError::InvalidField {
                index,
                reason: "units must be ASCII and at most 9 bytes".into(),
            });
        }
        if !field.scale.is_finite() || field.scale == 0.0 || !field.transform.is_finite() {
            return Err(DatalogError::InvalidField {
                index,
                reason: "scale must be finite/non-zero and transform finite".into(),
            });
        }
    }
    Ok(())
}

fn put_c_string(target: &mut [u8], text: &str) {
    target[..text.len()].copy_from_slice(text.as_bytes());
}

fn read_c_string(bytes: &[u8], index: usize, label: &str) -> Result<String> {
    let nul = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    if !bytes[..nul].is_ascii() {
        return Err(DatalogError::InvalidField {
            index,
            reason: format!("{label} is not ASCII"),
        });
    }
    Ok(String::from_utf8(bytes[..nul].to_vec()).expect("ASCII is UTF-8"))
}

fn write_marker(writer: &mut impl Write, marker: &Marker, index: usize) -> Result<()> {
    if !marker.text.is_ascii() || marker.text.len() > 49 {
        return Err(DatalogError::InvalidRecord {
            index,
            reason: "marker text must be ASCII and at most 49 bytes".into(),
        });
    }
    let mut bytes = [0u8; MARKER_LEN];
    bytes[0] = 1;
    bytes[1] = marker.counter;
    bytes[2..4].copy_from_slice(&(marker.timestamp_10us as u16).to_be_bytes());
    bytes[4..4 + marker.text.len()].copy_from_slice(marker.text.as_bytes());
    writer.write_all(&bytes)?;
    Ok(())
}

fn read_marker_text(bytes: &[u8], index: usize) -> Result<String> {
    let nul = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    if !bytes[..nul].is_ascii() {
        return Err(DatalogError::InvalidRecord {
            index,
            reason: "marker text is not ASCII".into(),
        });
    }
    Ok(String::from_utf8(bytes[..nul].to_vec()).expect("ASCII is UTF-8"))
}

fn write_raw(writer: &mut impl Write, field: &Field, value: f64) -> Result<()> {
    let raw = value / f64::from(field.scale) - f64::from(field.transform);
    let range_error = || DatalogError::ValueOutOfRange {
        field: field.name.clone(),
        value,
    };
    match field.field_type {
        FieldType::U8 => {
            writer.write_all(&[(checked_integer(raw, 0.0, u8::MAX as f64, range_error)? as u8)])?
        }
        FieldType::I8 => {
            writer.write_all(&[
                (checked_integer(raw, i8::MIN as f64, i8::MAX as f64, range_error)? as i8) as u8,
            ])?
        }
        FieldType::U16 => writer.write_all(
            &(checked_integer(raw, 0.0, u16::MAX as f64, range_error)? as u16).to_be_bytes(),
        )?,
        FieldType::I16 => writer.write_all(
            &(checked_integer(raw, i16::MIN as f64, i16::MAX as f64, range_error)? as i16)
                .to_be_bytes(),
        )?,
        FieldType::U32 => writer.write_all(
            &(checked_integer(raw, 0.0, u32::MAX as f64, range_error)? as u32).to_be_bytes(),
        )?,
        FieldType::I32 => writer.write_all(
            &(checked_integer(raw, i32::MIN as f64, i32::MAX as f64, range_error)? as i32)
                .to_be_bytes(),
        )?,
        FieldType::I64 => writer.write_all(
            &(checked_integer(raw, i64::MIN as f64, i64::MAX as f64, range_error)? as i64)
                .to_be_bytes(),
        )?,
        FieldType::F32 => writer.write_all(&(raw as f32).to_bits().to_be_bytes())?,
    }
    Ok(())
}

fn checked_integer(
    value: f64,
    min: f64,
    max: f64,
    error: impl FnOnce() -> DatalogError,
) -> Result<f64> {
    let rounded = value.round();
    if !rounded.is_finite() || rounded < min || rounded > max {
        Err(error())
    } else {
        Ok(rounded)
    }
}

fn read_raw(kind: FieldType, bytes: &[u8]) -> f64 {
    match kind {
        FieldType::U8 => f64::from(bytes[0]),
        FieldType::I8 => f64::from(bytes[0] as i8),
        FieldType::U16 => f64::from(be_u16(bytes)),
        FieldType::I16 => f64::from(be_u16(bytes) as i16),
        FieldType::U32 => f64::from(be_u32(bytes)),
        FieldType::I32 => f64::from(be_u32(bytes) as i32),
        FieldType::I64 => i64::from_be_bytes(bytes.try_into().expect("field width")) as f64,
        FieldType::F32 => f64::from(f32::from_bits(be_u32(bytes))),
    }
}

fn be_u16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes(bytes.try_into().expect("two-byte slice"))
}

fn be_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().expect("four-byte slice"))
}

fn unwrap_timestamp(raw: u16, last: &mut Option<u16>, wrap: &mut u64) -> u64 {
    if last.is_some_and(|previous| raw < previous) {
        *wrap += 65_536;
    }
    *last = Some(raw);
    *wrap + u64::from(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Log {
        let mut log = Log::new(vec![
            Field {
                name: "rpm".into(),
                units: "RPM".into(),
                field_type: FieldType::U16,
                display_style: 0,
                scale: 1.0,
                transform: 0.0,
                digits: 0,
            },
            Field::float("afr", "AFR"),
        ]);
        log.started_unix = 1_700_000_000;
        log.entries.push(LogEntry::Record(Record {
            counter: 7,
            timestamp_10us: 1234,
            values: vec![3210.0, f64::NAN],
        }));
        log.entries.push(LogEntry::Marker(Marker {
            counter: 8,
            timestamp_10us: 1235,
            text: "pull starts".into(),
        }));
        log
    }

    #[test]
    fn v1_roundtrip_preserves_header_fields_records_and_marker() {
        let log = fixture();
        let mut bytes = Vec::new();
        write_mlg_v1(&log, &mut bytes).unwrap();
        assert_eq!(&bytes[..6], MAGIC);
        assert_eq!(be_u16(&bytes[6..8]), 1);
        assert_eq!(be_u16(&bytes[12..14]), 0);
        assert_eq!(be_u32(&bytes[14..18]) as usize, HEADER_LEN + 2 * 55);
        let decoded = read_mlg_v1(bytes.as_slice()).unwrap();
        assert_eq!(decoded.fields, log.fields);
        assert_eq!(decoded.started_unix, log.started_unix);
        assert_eq!(decoded.entries.len(), 2);
        assert!(decoded.records().next().unwrap().values[1].is_nan());
        assert_eq!(decoded.markers().next().unwrap().text, "pull starts");
    }

    #[test]
    fn malformed_files_report_specific_errors() {
        assert!(matches!(
            read_mlg_v1(&b"short"[..]),
            Err(DatalogError::Truncated("header"))
        ));
        let mut bytes = Vec::new();
        write_mlg_v1(&fixture(), &mut bytes).unwrap();
        bytes[6..8].copy_from_slice(&2u16.to_be_bytes());
        assert!(matches!(
            read_mlg_v1(bytes.as_slice()),
            Err(DatalogError::UnsupportedVersion(2))
        ));
    }

    #[test]
    fn truncated_record_is_rejected() {
        let mut bytes = Vec::new();
        write_mlg_v1(&fixture(), &mut bytes).unwrap();
        bytes.pop();
        assert!(matches!(
            read_mlg_v1(bytes.as_slice()),
            Err(DatalogError::Truncated("marker"))
        ));
    }

    #[test]
    fn timestamp_wrap_is_unwrapped_for_playback() {
        let mut log = Log::new(vec![Field::float("rpm", "RPM")]);
        for timestamp_10us in [65_530, 65_540] {
            log.entries.push(LogEntry::Record(Record {
                counter: 0,
                timestamp_10us,
                values: vec![1000.0],
            }));
        }
        let mut bytes = Vec::new();
        write_mlg_v1(&log, &mut bytes).unwrap();
        let decoded = read_mlg_v1(bytes.as_slice()).unwrap();
        let times: Vec<_> = decoded
            .records()
            .map(|record| record.timestamp_10us)
            .collect();
        assert_eq!(times, vec![65_530, 65_540]);
    }

    #[test]
    fn every_published_scalar_type_roundtrips() {
        let kinds = [
            FieldType::U8,
            FieldType::I8,
            FieldType::U16,
            FieldType::I16,
            FieldType::U32,
            FieldType::I32,
            FieldType::I64,
            FieldType::F32,
        ];
        let values = [
            200.0,
            -100.0,
            50_000.0,
            -20_000.0,
            3_000_000_000.0,
            -2_000_000_000.0,
            -9_000_000.0,
            12.5,
        ];
        let fields = kinds
            .iter()
            .enumerate()
            .map(|(index, kind)| Field {
                name: format!("f{index}"),
                units: String::new(),
                field_type: *kind,
                display_style: 0,
                scale: 1.0,
                transform: 0.0,
                digits: 0,
            })
            .collect();
        let mut log = Log::new(fields);
        log.entries.push(LogEntry::Record(Record {
            counter: 0,
            timestamp_10us: 0,
            values: values.to_vec(),
        }));
        let mut bytes = Vec::new();
        write_mlg_v1(&log, &mut bytes).unwrap();
        let decoded = read_mlg_v1(bytes.as_slice()).unwrap();
        assert_eq!(decoded.records().next().unwrap().values, values);
    }

    #[test]
    fn corrupt_record_crc_is_a_typed_error() {
        let mut bytes = Vec::new();
        write_mlg_v1(&fixture(), &mut bytes).unwrap();
        let data_start = be_u32(&bytes[14..18]) as usize;
        bytes[data_start + 4] ^= 0x01;
        assert!(matches!(
            read_mlg_v1(bytes.as_slice()),
            Err(DatalogError::InvalidRecord { .. })
        ));
    }
}
