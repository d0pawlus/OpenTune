// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::{Read, Write};

use crate::{DatalogError, Field, Log, LogEntry, Record, Result};

/// Write conventional deterministic CSV: `Time` (seconds), then one column per
/// field. Markers have no standard CSV representation and are intentionally
/// omitted. CSV has no representation for "missing" other than an empty
/// cell, so every non-finite data value (NaN, +inf, -inf) is emitted as one.
pub fn write_csv<W: Write>(log: &Log, mut writer: W) -> Result<()> {
    write_cell(&mut writer, "Time")?;
    for field in &log.fields {
        writer.write_all(b",")?;
        write_cell(&mut writer, &field.name)?;
    }
    writer.write_all(b"\n")?;

    for record in log.records() {
        write!(writer, "{}", record.timestamp_10us as f64 / 100_000.0)?;
        for index in 0..log.fields.len() {
            writer.write_all(b",")?;
            let value = record.values.get(index).copied().unwrap_or(f64::NAN);
            // CSV has one representation for "missing": an empty cell. NaN
            // and ±inf all collapse to it — the reader parses an empty cell
            // back to NaN, the same "missing" representation the MLG path
            // uses (M5 review LOW-csv).
            if value.is_finite() {
                write!(writer, "{value}")?;
            }
        }
        writer.write_all(b"\n")?;
    }
    Ok(())
}

/// Read CSV written by OpenTune and ordinary RFC-4180-style CSV files.
/// Quoted commas, quotes and newlines are supported. Empty cells and
/// case-insensitive `NaN` become `f64::NAN`.
pub fn read_csv<R: Read>(mut reader: R) -> Result<Log> {
    let mut input = String::new();
    reader.read_to_string(&mut input)?;
    let rows = parse_rows(&input)?;
    let Some(header) = rows.first() else {
        return Err(DatalogError::InvalidCsv {
            line: 1,
            reason: "missing header".into(),
        });
    };
    if header.is_empty() || !header[0].eq_ignore_ascii_case("time") {
        return Err(DatalogError::InvalidCsv {
            line: 1,
            reason: "first column must be Time".into(),
        });
    }
    let fields = header[1..]
        .iter()
        .map(|name| Field::float(name, ""))
        .collect();
    let mut log = Log::new(fields);
    for (row_index, row) in rows.iter().enumerate().skip(1) {
        if row.len() == 1 && row[0].is_empty() {
            continue;
        }
        if row.len() > header.len() {
            return Err(DatalogError::InvalidCsv {
                line: row_index + 1,
                reason: format!("{} columns, expected at most {}", row.len(), header.len()),
            });
        }
        let seconds = parse_number(
            row.first().map_or("", String::as_str),
            row_index + 1,
            "Time",
        )?;
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(DatalogError::InvalidCsv {
                line: row_index + 1,
                reason: "Time must be a finite non-negative number".into(),
            });
        }
        let ticks = (seconds * 100_000.0).round();
        let timestamp_10us = ticks as u64;
        let mut values = Vec::with_capacity(header.len().saturating_sub(1));
        for (column, name) in header.iter().enumerate().skip(1) {
            let text = row.get(column).map_or("", String::as_str);
            values.push(parse_number(text, row_index + 1, name)?);
        }
        log.entries.push(LogEntry::Record(Record {
            counter: ((row_index - 1) % 255) as u8,
            timestamp_10us,
            values,
        }));
    }
    Ok(log)
}

fn parse_number(text: &str, line: usize, column: &str) -> Result<f64> {
    let text = text.trim();
    if text.is_empty() || text.eq_ignore_ascii_case("nan") {
        return Ok(f64::NAN);
    }
    text.parse().map_err(|_| DatalogError::InvalidCsv {
        line,
        reason: format!("`{text}` is not a number in column `{column}`"),
    })
}

fn write_cell(writer: &mut impl Write, value: &str) -> Result<()> {
    if value
        .bytes()
        .any(|b| matches!(b, b',' | b'"' | b'\r' | b'\n'))
    {
        writer.write_all(b"\"")?;
        writer.write_all(value.replace('"', "\"\"").as_bytes())?;
        writer.write_all(b"\"")?;
    } else {
        writer.write_all(value.as_bytes())?;
    }
    Ok(())
}

fn parse_rows(input: &str) -> Result<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut chars = input.chars().peekable();
    let mut quoted = false;
    let mut line = 1;
    while let Some(ch) = chars.next() {
        if quoted {
            match ch {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    cell.push('"');
                }
                '"' => quoted = false,
                '\n' => {
                    line += 1;
                    cell.push(ch);
                }
                _ => cell.push(ch),
            }
        } else {
            match ch {
                '"' if cell.is_empty() => quoted = true,
                '"' => {
                    return Err(DatalogError::InvalidCsv {
                        line,
                        reason: "quote inside an unquoted cell".into(),
                    })
                }
                ',' => row.push(std::mem::take(&mut cell)),
                '\n' => {
                    row.push(std::mem::take(&mut cell));
                    rows.push(std::mem::take(&mut row));
                    line += 1;
                }
                '\r' if chars.peek() == Some(&'\n') => {}
                '\r' => {
                    row.push(std::mem::take(&mut cell));
                    rows.push(std::mem::take(&mut row));
                    line += 1;
                }
                _ => cell.push(ch),
            }
        }
    }
    if quoted {
        return Err(DatalogError::InvalidCsv {
            line,
            reason: "unterminated quoted cell".into(),
        });
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_and_missing_roundtrip() {
        let csv = "\"Time\",\"rpm,engine\",\"note\"\"value\"\r\n0,1000,\r\n0.1,NaN,2\r\n";
        let log = read_csv(csv.as_bytes()).unwrap();
        assert_eq!(log.fields[0].name, "rpm,engine");
        assert_eq!(log.fields[1].name, "note\"value");
        assert!(log.records().next().unwrap().values[1].is_nan());
        let mut encoded = Vec::new();
        write_csv(&log, &mut encoded).unwrap();
        let decoded = read_csv(encoded.as_slice()).unwrap();
        assert_eq!(decoded.fields, log.fields);
        assert!(decoded.records().nth(1).unwrap().values[0].is_nan());
    }

    #[test]
    fn malformed_quote_is_typed_error() {
        let error = read_csv("Time,rpm\n0,12\"3\n".as_bytes()).unwrap_err();
        assert!(matches!(error, DatalogError::InvalidCsv { line: 2, .. }));
    }

    /// M5 review LOW-csv: non-finite data cells (NaN, +inf, -inf) all
    /// collapse to the CSV format's one "missing" representation — an empty
    /// cell, the same value `write_csv` already used for NaN — round-tripping
    /// back to NaN on read (the MLG path's own "missing" representation, per
    /// `Record::values`'s doc comment).
    #[test]
    fn non_finite_data_cells_round_trip_as_missing() {
        let mut log = Log::new(vec![Field::float("a", ""), Field::float("b", "")]);
        log.entries.push(LogEntry::Record(Record {
            counter: 0,
            timestamp_10us: 0,
            values: vec![f64::NAN, f64::INFINITY],
        }));
        log.entries.push(LogEntry::Record(Record {
            counter: 1,
            timestamp_10us: 1,
            values: vec![f64::NEG_INFINITY, 1.5],
        }));

        let mut encoded = Vec::new();
        write_csv(&log, &mut encoded).unwrap();
        let text = String::from_utf8(encoded.clone()).unwrap();
        // Every non-finite cell serializes as empty — no literal "inf"/"NaN"
        // token reaches the file.
        assert!(!text.to_ascii_lowercase().contains("inf"));

        let decoded = read_csv(encoded.as_slice()).unwrap();
        let rows: Vec<_> = decoded.records().collect();
        assert!(rows[0].values[0].is_nan(), "NaN must round-trip as NaN");
        assert!(
            rows[0].values[1].is_nan(),
            "+inf must round-trip as the missing representation (NaN)"
        );
        assert!(
            rows[1].values[0].is_nan(),
            "-inf must round-trip as the missing representation (NaN)"
        );
        assert_eq!(rows[1].values[1], 1.5, "finite values are unaffected");
    }
}
