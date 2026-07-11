// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{validate_samples, LogAnalysisError, SampleSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    LessThan,
    LessOrEqual,
    GreaterThan,
    GreaterOrEqual,
    Equal,
    NotEqual,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SampleFilter {
    pub channel: String,
    pub comparison: Comparison,
    pub value: f64,
    /// Stable machine-readable reason shown in audit output.
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LogStatsParams {
    /// Empty means every input channel, in input order.
    pub channels: Vec<String>,
    /// A matching predicate rejects the row. First match wins.
    pub reject_when: Vec<SampleFilter>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FilterReasonCount {
    pub reason: String,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StatsFilterDecision {
    pub row: u32,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SummaryStat {
    pub channel: String,
    pub finite_count: u32,
    pub missing_count: u32,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub mean: Option<f64>,
    /// Population standard deviation.
    pub std_dev: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogStatsReport {
    pub total_rows: u32,
    pub accepted_rows: u32,
    pub stats: Vec<SummaryStat>,
    pub filtered: Vec<FilterReasonCount>,
    /// One auditable decision per rejected row, in row order.
    pub decisions: Vec<StatsFilterDecision>,
}

pub fn log_stats(
    samples: &SampleSet,
    params: &LogStatsParams,
) -> Result<LogStatsReport, LogAnalysisError> {
    validate_samples(samples)?;
    let selected: Vec<(&str, usize)> = if params.channels.is_empty() {
        samples
            .columns
            .iter()
            .enumerate()
            .map(|(index, name)| (name.as_str(), index))
            .collect()
    } else {
        params
            .channels
            .iter()
            .map(|name| {
                samples
                    .column(name)
                    .map(|index| (name.as_str(), index))
                    .ok_or_else(|| LogAnalysisError::MissingChannel(name.clone()))
            })
            .collect::<Result<_, _>>()?
    };
    let filters: Vec<(&SampleFilter, usize)> = params
        .reject_when
        .iter()
        .map(|filter| {
            if !filter.value.is_finite() {
                return Err(LogAnalysisError::InvalidParameter(format!(
                    "filter `{}` threshold must be finite",
                    filter.reason
                )));
            }
            samples
                .column(&filter.channel)
                .map(|index| (filter, index))
                .ok_or_else(|| LogAnalysisError::MissingChannel(filter.channel.clone()))
        })
        .collect::<Result<_, _>>()?;

    let mut accepted = vec![true; samples.len()];
    let mut counts = vec![0u32; filters.len()];
    let mut decisions = Vec::new();
    for (row_index, row) in samples.rows.iter().enumerate() {
        for (filter_index, (filter, column)) in filters.iter().enumerate() {
            if matches_filter(row[*column], filter.comparison, filter.value) {
                accepted[row_index] = false;
                counts[filter_index] = counts[filter_index].saturating_add(1);
                decisions.push(StatsFilterDecision {
                    row: saturating_u32(row_index),
                    reason: filter.reason.clone(),
                });
                break;
            }
        }
    }

    let stats = selected
        .into_iter()
        .map(|(name, column)| summarize(name, column, samples, &accepted))
        .collect();
    Ok(LogStatsReport {
        total_rows: saturating_u32(samples.len()),
        accepted_rows: saturating_u32(accepted.iter().filter(|value| **value).count()),
        stats,
        filtered: filters
            .iter()
            .zip(counts)
            .map(|((filter, _), count)| FilterReasonCount {
                reason: filter.reason.clone(),
                count,
            })
            .collect(),
        decisions,
    })
}

fn summarize(name: &str, column: usize, samples: &SampleSet, accepted: &[bool]) -> SummaryStat {
    // Welford's algorithm is deterministic in fixed row order and stable for
    // large offsets.
    let mut count = 0u32;
    let mut missing = 0u32;
    let mut mean = 0.0;
    let mut m2 = 0.0;
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for (row, keep) in samples.rows.iter().zip(accepted) {
        if !keep {
            continue;
        }
        let value = row[column];
        if !value.is_finite() {
            missing = missing.saturating_add(1);
            continue;
        }
        count = count.saturating_add(1);
        let delta = value - mean;
        mean += delta / f64::from(count);
        m2 += delta * (value - mean);
        min = min.min(value);
        max = max.max(value);
    }
    SummaryStat {
        channel: name.to_owned(),
        finite_count: count,
        missing_count: missing,
        min: (count > 0).then_some(min),
        max: (count > 0).then_some(max),
        mean: (count > 0).then_some(mean),
        std_dev: (count > 0).then_some((m2 / f64::from(count)).sqrt()),
    }
}

fn matches_filter(value: f64, comparison: Comparison, threshold: f64) -> bool {
    if !value.is_finite() {
        return false;
    }
    match comparison {
        Comparison::LessThan => value < threshold,
        Comparison::LessOrEqual => value <= threshold,
        Comparison::GreaterThan => value > threshold,
        Comparison::GreaterOrEqual => value >= threshold,
        Comparison::Equal => value == threshold,
        Comparison::NotEqual => value != threshold,
    }
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples() -> SampleSet {
        SampleSet {
            columns: vec!["rpm".into(), "afr".into()],
            t_ms: vec![0.0, 10.0, 20.0, 30.0],
            rows: vec![
                vec![1000.0, 14.0],
                vec![2000.0, 15.0],
                vec![300.0, 99.0],
                vec![3000.0, f64::NAN],
            ],
        }
    }

    #[test]
    fn filters_are_auditable_and_first_match_wins() {
        let params = LogStatsParams {
            channels: vec!["afr".into()],
            reject_when: vec![
                SampleFilter {
                    channel: "rpm".into(),
                    comparison: Comparison::LessThan,
                    value: 500.0,
                    reason: "engine_not_running".into(),
                },
                SampleFilter {
                    channel: "afr".into(),
                    comparison: Comparison::GreaterThan,
                    value: 20.0,
                    reason: "afr_outlier".into(),
                },
            ],
        };
        let a = log_stats(&samples(), &params).unwrap();
        let b = log_stats(&samples(), &params).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.accepted_rows, 3);
        assert_eq!(a.filtered[0].count, 1);
        assert_eq!(a.filtered[1].count, 0);
        assert_eq!(a.decisions[0].reason, "engine_not_running");
        assert_eq!(a.stats[0].finite_count, 2);
        assert_eq!(a.stats[0].missing_count, 1);
        assert_eq!(a.stats[0].mean, Some(14.5));
    }
}
