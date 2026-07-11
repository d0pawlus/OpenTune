// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{validate_samples, LogAnalysisError, SampleSet};

#[derive(Debug, Clone, PartialEq)]
pub struct SensorThreshold {
    pub channel: String,
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnomalyThresholds {
    pub sensors: Vec<SensorThreshold>,
    pub afr_channel: String,
    pub lean_afr: f64,
    pub lean_min_rpm: f64,
    pub rpm_channel: String,
    pub load_channel: String,
    pub lean_min_load: f64,
    pub knock_channel: String,
    pub knock_threshold: f64,
    pub knock_min_rpm: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnomalyKind {
    SensorDropout,
    LeanSpike,
    Knock,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Anomaly {
    pub row: u32,
    pub t_ms: f64,
    pub kind: AnomalyKind,
    pub channel: String,
    /// `None` represents a missing/non-finite sensor value.
    pub value: Option<f64>,
    pub threshold: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnomalyReport {
    pub inspected_rows: u32,
    pub anomalies: Vec<Anomaly>,
}

pub fn detect_anomaly(
    samples: &SampleSet,
    thresholds: &AnomalyThresholds,
) -> Result<AnomalyReport, LogAnalysisError> {
    validate_samples(samples)?;
    validate_thresholds(thresholds)?;
    let sensors = thresholds
        .sensors
        .iter()
        .map(|sensor| {
            samples
                .column(&sensor.channel)
                .map(|index| (sensor, index))
                .ok_or_else(|| LogAnalysisError::MissingChannel(sensor.channel.clone()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let afr = required_column(samples, &thresholds.afr_channel)?;
    let rpm = required_column(samples, &thresholds.rpm_channel)?;
    let load = required_column(samples, &thresholds.load_channel)?;
    let knock = required_column(samples, &thresholds.knock_channel)?;

    let mut anomalies = Vec::new();
    for (row_index, row) in samples.rows.iter().enumerate() {
        for (sensor, column) in &sensors {
            let value = row[*column];
            if !value.is_finite() || value < sensor.min || value > sensor.max {
                anomalies.push(Anomaly {
                    row: saturating_u32(row_index),
                    t_ms: samples.t_ms[row_index],
                    kind: AnomalyKind::SensorDropout,
                    channel: sensor.channel.clone(),
                    value: value.is_finite().then_some(value),
                    threshold: format!("finite and {}..={}", sensor.min, sensor.max),
                });
            }
        }
        let rpm_value = row[rpm];
        let load_value = row[load];
        let afr_value = row[afr];
        if rpm_value.is_finite()
            && load_value.is_finite()
            && afr_value.is_finite()
            && rpm_value >= thresholds.lean_min_rpm
            && load_value >= thresholds.lean_min_load
            && afr_value >= thresholds.lean_afr
        {
            anomalies.push(Anomaly {
                row: saturating_u32(row_index),
                t_ms: samples.t_ms[row_index],
                kind: AnomalyKind::LeanSpike,
                channel: thresholds.afr_channel.clone(),
                value: Some(afr_value),
                threshold: format!(
                    "AFR >= {}; RPM >= {}; load >= {}",
                    thresholds.lean_afr, thresholds.lean_min_rpm, thresholds.lean_min_load
                ),
            });
        }
        let knock_value = row[knock];
        if rpm_value.is_finite()
            && knock_value.is_finite()
            && rpm_value >= thresholds.knock_min_rpm
            && knock_value >= thresholds.knock_threshold
        {
            anomalies.push(Anomaly {
                row: saturating_u32(row_index),
                t_ms: samples.t_ms[row_index],
                kind: AnomalyKind::Knock,
                channel: thresholds.knock_channel.clone(),
                value: Some(knock_value),
                threshold: format!(
                    "knock >= {}; RPM >= {}",
                    thresholds.knock_threshold, thresholds.knock_min_rpm
                ),
            });
        }
    }
    Ok(AnomalyReport {
        inspected_rows: saturating_u32(samples.len()),
        anomalies,
    })
}

fn validate_thresholds(thresholds: &AnomalyThresholds) -> Result<(), LogAnalysisError> {
    for sensor in &thresholds.sensors {
        if !sensor.min.is_finite() || !sensor.max.is_finite() || sensor.min > sensor.max {
            return Err(LogAnalysisError::InvalidParameter(format!(
                "invalid sensor range for `{}`",
                sensor.channel
            )));
        }
    }
    for (name, value) in [
        ("lean_afr", thresholds.lean_afr),
        ("lean_min_rpm", thresholds.lean_min_rpm),
        ("lean_min_load", thresholds.lean_min_load),
        ("knock_threshold", thresholds.knock_threshold),
        ("knock_min_rpm", thresholds.knock_min_rpm),
    ] {
        if !value.is_finite() {
            return Err(LogAnalysisError::InvalidParameter(format!(
                "{name} must be finite"
            )));
        }
    }
    Ok(())
}

fn required_column(samples: &SampleSet, name: &str) -> Result<usize, LogAnalysisError> {
    samples
        .column(name)
        .ok_or_else(|| LogAnalysisError::MissingChannel(name.to_owned()))
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thresholds() -> AnomalyThresholds {
        AnomalyThresholds {
            sensors: vec![SensorThreshold {
                channel: "clt".into(),
                min: -40.0,
                max: 150.0,
            }],
            afr_channel: "afr".into(),
            lean_afr: 16.0,
            lean_min_rpm: 2000.0,
            rpm_channel: "rpm".into(),
            load_channel: "map".into(),
            lean_min_load: 80.0,
            knock_channel: "knock".into(),
            knock_threshold: 3.0,
            knock_min_rpm: 2500.0,
        }
    }

    #[test]
    fn detects_all_three_classes_without_hidden_thresholds() {
        let samples = SampleSet {
            columns: vec![
                "clt".into(),
                "afr".into(),
                "rpm".into(),
                "map".into(),
                "knock".into(),
            ],
            t_ms: vec![0.0, 40.0],
            rows: vec![
                vec![f64::NAN, 14.7, 1000.0, 30.0, 0.0],
                vec![90.0, 17.2, 3500.0, 100.0, 4.0],
            ],
        };
        let first = detect_anomaly(&samples, &thresholds()).unwrap();
        let second = detect_anomaly(&samples, &thresholds()).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.anomalies.len(), 3);
        assert_eq!(first.anomalies[0].kind, AnomalyKind::SensorDropout);
        assert_eq!(first.anomalies[1].kind, AnomalyKind::LeanSpike);
        assert_eq!(first.anomalies[2].kind, AnomalyKind::Knock);
    }
}
