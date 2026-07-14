// SPDX-License-Identifier: GPL-3.0-or-later

use opentune_analysis::{
    AnomalyKind, AnomalyThresholds, LogStatsParams, SampleFilter, SensorThreshold,
    VirtualDynoParams,
};
use opentune_datalog::{Log, LogEntry};

use crate::dto::*;

pub const MAX_LOG_SLICE: u32 = 100_000;

pub fn summary(log: &Log) -> LogSummaryDto {
    let records = log.records().count();
    LogSummaryDto {
        fields: log
            .fields
            .iter()
            .map(|field| LogFieldDto {
                name: field.name.clone(),
                units: field.units.clone(),
            })
            .collect(),
        record_count: saturating_u32(records),
        marker_count: saturating_u32(log.markers().count()),
        duration_ms: log
            .records()
            .last()
            .map_or(0.0, |record| record.timestamp_10us as f64 / 100.0),
    }
}

pub fn slice(log: &Log, offset: u32, limit: u32) -> Result<LogDataDto, String> {
    if limit == 0 || limit > MAX_LOG_SLICE {
        return Err(format!("limit must be in 1..={MAX_LOG_SLICE}"));
    }
    let records: Vec<_> = log.records().collect();
    let start = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(records.len());
    let end = start.saturating_add(limit as usize).min(records.len());
    let selected = &records[start..end];
    let t_ms = selected
        .iter()
        .map(|record| record.timestamp_10us as f64 / 100.0)
        .collect();
    let columns = (0..log.fields.len())
        .map(|column| {
            selected
                .iter()
                .map(|record| {
                    record
                        .values
                        .get(column)
                        .copied()
                        .filter(|value| value.is_finite())
                })
                .collect()
        })
        .collect();
    let mut record_index = 0usize;
    let mut markers = Vec::new();
    // M5 review LOW-marker: a marker sitting exactly at a page's upper
    // bound (`record_index == end`) must belong to exactly one page. Every
    // page's range is `[start, end)` (half-open) EXCEPT the real last page
    // (a non-empty page reaching the end of the log), which also claims
    // markers trailing the very last record — there is no "next page" to
    // hand them to. `start < end` excludes an empty overflow request (e.g.
    // `offset == total_records`) from wrongly claiming that role too.
    let is_last_page = start < end && end >= records.len();
    for entry in &log.entries {
        match entry {
            LogEntry::Record(_) => record_index += 1,
            LogEntry::Marker(marker)
                if record_index >= start && (record_index < end || is_last_page) =>
            {
                markers.push(MarkerDto {
                    record_index: saturating_u32(record_index),
                    t_ms: marker.timestamp_10us as f64 / 100.0,
                    text: marker.text.clone(),
                });
            }
            LogEntry::Marker(_) => {}
        }
    }
    Ok(LogDataDto {
        offset,
        total_records: saturating_u32(records.len()),
        t_ms,
        columns,
        markers,
    })
}

pub fn to_samples(log: &Log) -> opentune_analysis::SampleSet {
    opentune_analysis::SampleSet {
        columns: log.fields.iter().map(|field| field.name.clone()).collect(),
        t_ms: log
            .records()
            .map(|record| record.timestamp_10us as f64 / 100.0)
            .collect(),
        rows: log.records().map(|record| record.values.clone()).collect(),
    }
}

pub fn stats_params(dto: LogStatsParamsDto) -> LogStatsParams {
    LogStatsParams {
        channels: dto.channels,
        reject_when: dto
            .reject_when
            .into_iter()
            .map(|filter| SampleFilter {
                channel: filter.channel,
                comparison: filter.comparison.into(),
                value: filter.value,
                reason: filter.reason,
            })
            .collect(),
    }
}

pub fn stats_report(report: opentune_analysis::LogStatsReport) -> LogStatsReportDto {
    LogStatsReportDto {
        total_rows: report.total_rows,
        accepted_rows: report.accepted_rows,
        stats: report
            .stats
            .into_iter()
            .map(|stat| SummaryStatDto {
                channel: stat.channel,
                finite_count: stat.finite_count,
                missing_count: stat.missing_count,
                min: stat.min,
                max: stat.max,
                mean: stat.mean,
                std_dev: stat.std_dev,
            })
            .collect(),
        filtered: report
            .filtered
            .into_iter()
            .map(|reason| ReasonCountDto {
                reason: reason.reason,
                count: reason.count,
            })
            .collect(),
        decisions: report
            .decisions
            .into_iter()
            .map(|decision| FilterDecisionDto {
                row: decision.row,
                reason: decision.reason,
            })
            .collect(),
    }
}

pub fn anomaly_params(dto: AnomalyThresholdsDto) -> AnomalyThresholds {
    AnomalyThresholds {
        sensors: dto
            .sensors
            .into_iter()
            .map(|sensor| SensorThreshold {
                channel: sensor.channel,
                min: sensor.min,
                max: sensor.max,
            })
            .collect(),
        afr_channel: dto.afr_channel,
        lean_afr: dto.lean_afr,
        lean_min_rpm: dto.lean_min_rpm,
        rpm_channel: dto.rpm_channel,
        load_channel: dto.load_channel,
        lean_min_load: dto.lean_min_load,
        knock_channel: dto.knock_channel,
        knock_threshold: dto.knock_threshold,
        knock_min_rpm: dto.knock_min_rpm,
    }
}

pub fn anomaly_report(report: opentune_analysis::AnomalyReport) -> AnomalyReportDto {
    AnomalyReportDto {
        inspected_rows: report.inspected_rows,
        anomalies: report
            .anomalies
            .into_iter()
            .map(|anomaly| AnomalyDto {
                row: anomaly.row,
                t_ms: anomaly.t_ms,
                kind: match anomaly.kind {
                    AnomalyKind::SensorDropout => AnomalyKindDto::SensorDropout,
                    AnomalyKind::LeanSpike => AnomalyKindDto::LeanSpike,
                    AnomalyKind::Knock => AnomalyKindDto::Knock,
                },
                channel: anomaly.channel,
                value: anomaly.value,
                threshold: anomaly.threshold,
            })
            .collect(),
    }
}

pub fn dyno_params(dto: VirtualDynoParamsDto) -> VirtualDynoParams {
    VirtualDynoParams {
        speed_channel: dto.speed_channel,
        rpm_channel: dto.rpm_channel,
        mass_kg: dto.mass_kg,
        drag_coefficient: dto.drag_coefficient,
        frontal_area_m2: dto.frontal_area_m2,
        rolling_resistance: dto.rolling_resistance,
        drivetrain_loss: dto.drivetrain_loss,
        smoothing_window: dto.smoothing_window,
        air_density_kg_m3: dto.air_density_kg_m3,
    }
}

pub fn dyno_report(report: opentune_analysis::VirtualDynoReport) -> VirtualDynoReportDto {
    VirtualDynoReportDto {
        points: report
            .points
            .into_iter()
            .map(|point| DynoPointDto {
                row: point.row,
                t_ms: point.t_ms,
                speed_m_s: point.speed_m_s,
                rpm: point.rpm,
                acceleration_m_s2: point.acceleration_m_s2,
                inertial_force_n: point.inertial_force_n,
                aero_force_n: point.aero_force_n,
                rolling_force_n: point.rolling_force_n,
                wheel_power_w: point.wheel_power_w,
                wheel_hp: point.wheel_hp,
                estimated_engine_power_w: point.estimated_engine_power_w,
                estimated_engine_hp: point.estimated_engine_hp,
                estimated_engine_torque_nm: point.estimated_engine_torque_nm,
            })
            .collect(),
        conditions: report
            .conditions
            .into_iter()
            .map(|condition| DynoConditionDto {
                row: condition.row,
                accepted: condition.accepted,
                reason: condition.reason,
            })
            .collect(),
        assumptions: report.assumptions,
    }
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentune_datalog::{Field, LogEntry, Marker, Record};

    #[test]
    fn slice_is_columnar_and_maps_nan_to_null() {
        let mut log = Log::new(vec![Field::float("rpm", "RPM")]);
        log.entries.push(LogEntry::Record(Record {
            counter: 0,
            timestamp_10us: 10,
            values: vec![f64::NAN],
        }));
        let dto = slice(&log, 0, 100).unwrap();
        assert_eq!(dto.columns, vec![vec![None]]);
        assert_eq!(dto.t_ms, vec![0.1]);
    }

    #[test]
    fn transfer_limit_is_enforced() {
        let log = Log::new(Vec::new());
        assert!(slice(&log, 0, MAX_LOG_SLICE + 1).is_err());
    }

    fn push_record(log: &mut Log, timestamp_10us: u64) {
        log.entries.push(LogEntry::Record(Record {
            counter: 0,
            timestamp_10us,
            values: vec![],
        }));
    }

    fn push_marker(log: &mut Log, timestamp_10us: u64, text: &str) {
        log.entries.push(LogEntry::Marker(Marker {
            counter: 0,
            timestamp_10us,
            text: text.to_string(),
        }));
    }

    /// M5 review LOW-marker: a marker sitting exactly on the boundary
    /// between two adjacent pages (5 records in, out of 10 total) must be
    /// emitted on exactly one of the two `slice` calls a paginating
    /// frontend would make — not both.
    #[test]
    fn marker_on_a_mid_log_page_boundary_is_emitted_exactly_once() {
        let mut log = Log::new(Vec::new());
        for i in 0..5u64 {
            push_record(&mut log, i);
        }
        push_marker(&mut log, 5, "boundary");
        for i in 5..10u64 {
            push_record(&mut log, i);
        }

        let first_page = slice(&log, 0, 5).unwrap();
        let second_page = slice(&log, 5, 5).unwrap();

        let total_markers = first_page.markers.len() + second_page.markers.len();
        assert_eq!(
            total_markers, 1,
            "boundary marker must appear on exactly one page: first={:?} second={:?}",
            first_page.markers, second_page.markers
        );
        assert!(
            second_page.markers.iter().any(|m| m.text == "boundary"),
            "the boundary marker belongs to the page that starts where it sits"
        );
    }

    /// A marker trailing the very last record (`record_index == total`) must
    /// land on the real last page, and must NOT be re-emitted by a
    /// subsequent empty "next page" request at `offset == total` — the same
    /// "exactly one page" contract at the tail of the log.
    #[test]
    fn trailing_marker_is_not_duplicated_onto_an_empty_next_page() {
        let mut log = Log::new(Vec::new());
        for i in 0..5u64 {
            push_record(&mut log, i);
        }
        push_marker(&mut log, 5, "trailing");

        let last_page = slice(&log, 0, 5).unwrap();
        let empty_next_page = slice(&log, 5, 5).unwrap();

        assert_eq!(
            last_page.markers.len(),
            1,
            "trailing marker on the last page"
        );
        assert!(
            empty_next_page.markers.is_empty(),
            "an empty overflow page must not repeat the trailing marker: {:?}",
            empty_next_page.markers
        );
    }
}
