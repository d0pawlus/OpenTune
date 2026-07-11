// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{validate_samples, LogAnalysisError, SampleSet};

const WATTS_PER_HP: f64 = 745.699_871_582_270_2;

#[derive(Debug, Clone, PartialEq)]
pub struct VirtualDynoParams {
    pub speed_channel: String,
    pub rpm_channel: String,
    pub mass_kg: f64,
    pub drag_coefficient: f64,
    pub frontal_area_m2: f64,
    pub rolling_resistance: f64,
    /// Fraction in `[0, 1)`, e.g. `0.15`.
    pub drivetrain_loss: f64,
    /// Trailing moving-average width. Must be at least one.
    pub smoothing_window: u32,
    pub air_density_kg_m3: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DynoPoint {
    pub row: u32,
    pub t_ms: f64,
    pub speed_m_s: f64,
    pub rpm: f64,
    pub acceleration_m_s2: f64,
    pub inertial_force_n: f64,
    pub aero_force_n: f64,
    pub rolling_force_n: f64,
    pub wheel_power_w: f64,
    pub wheel_hp: f64,
    pub estimated_engine_power_w: f64,
    pub estimated_engine_hp: f64,
    pub estimated_engine_torque_nm: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DynoCondition {
    pub row: u32,
    pub accepted: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VirtualDynoReport {
    pub points: Vec<DynoPoint>,
    pub conditions: Vec<DynoCondition>,
    pub assumptions: Vec<String>,
}

pub fn virtual_dyno(
    samples: &SampleSet,
    params: &VirtualDynoParams,
) -> Result<VirtualDynoReport, LogAnalysisError> {
    validate_samples(samples)?;
    validate_params(params)?;
    if samples.len() < 2 {
        return Err(LogAnalysisError::InsufficientData(
            "virtual dyno requires at least two samples".into(),
        ));
    }
    let speed_column = samples
        .column(&params.speed_channel)
        .ok_or_else(|| LogAnalysisError::MissingChannel(params.speed_channel.clone()))?;
    let rpm_column = samples
        .column(&params.rpm_channel)
        .ok_or_else(|| LogAnalysisError::MissingChannel(params.rpm_channel.clone()))?;
    let window = params.smoothing_window as usize;
    let speeds: Vec<f64> = samples.rows.iter().map(|row| row[speed_column]).collect();
    let smoothed: Vec<f64> = (0..speeds.len())
        .map(|index| trailing_mean(&speeds, index, window))
        .collect();
    let mut points = Vec::new();
    let mut conditions = vec![DynoCondition {
        row: 0,
        accepted: false,
        reason: "no preceding sample for acceleration".into(),
    }];
    for index in 1..samples.len() {
        let dt = (samples.t_ms[index] - samples.t_ms[index - 1]) / 1000.0;
        let speed = smoothed[index];
        let previous_speed = smoothed[index - 1];
        let rpm = samples.rows[index][rpm_column];
        let invalid_reason = if !dt.is_finite() || dt <= 0.0 {
            Some("timestamp is not strictly increasing")
        } else if !speed.is_finite() || !previous_speed.is_finite() {
            Some("speed is missing/non-finite in smoothing window")
        } else if speed < 0.0 {
            Some("speed is negative")
        } else if !rpm.is_finite() || rpm <= 0.0 {
            Some("RPM is missing/non-positive")
        } else {
            None
        };
        if let Some(reason) = invalid_reason {
            conditions.push(DynoCondition {
                row: saturating_u32(index),
                accepted: false,
                reason: reason.into(),
            });
            continue;
        }
        let acceleration = (speed - previous_speed) / dt;
        if acceleration <= 0.0 {
            conditions.push(DynoCondition {
                row: saturating_u32(index),
                accepted: false,
                reason: "non-positive acceleration (not a power pull)".into(),
            });
            continue;
        }
        let inertial_force = params.mass_kg * acceleration;
        let aero_force = 0.5
            * params.air_density_kg_m3
            * params.drag_coefficient
            * params.frontal_area_m2
            * speed
            * speed;
        let rolling_force = params.rolling_resistance * params.mass_kg * 9.806_65;
        let wheel_power = (inertial_force + aero_force + rolling_force) * speed;
        let engine_power = wheel_power / (1.0 - params.drivetrain_loss);
        let angular_velocity = rpm * std::f64::consts::TAU / 60.0;
        points.push(DynoPoint {
            row: saturating_u32(index),
            t_ms: samples.t_ms[index],
            speed_m_s: speed,
            rpm,
            acceleration_m_s2: acceleration,
            inertial_force_n: inertial_force,
            aero_force_n: aero_force,
            rolling_force_n: rolling_force,
            wheel_power_w: wheel_power,
            wheel_hp: wheel_power / WATTS_PER_HP,
            estimated_engine_power_w: engine_power,
            estimated_engine_hp: engine_power / WATTS_PER_HP,
            estimated_engine_torque_nm: engine_power / angular_velocity,
        });
        conditions.push(DynoCondition {
            row: saturating_u32(index),
            accepted: true,
            reason: "finite increasing timestamp, positive speed/RPM/acceleration".into(),
        });
    }
    Ok(VirtualDynoReport {
        points,
        conditions,
        assumptions: vec![
            "speed channel is metres per second; RPM is revolutions per minute".into(),
            "level road, still air, constant vehicle mass and coefficients".into(),
            format!(
                "air density = {} kg/m^3; gravity = 9.80665 m/s^2",
                params.air_density_kg_m3
            ),
            format!(
                "trailing moving-average smoothing window = {} samples",
                params.smoothing_window
            ),
            "wheel power includes inertial, aerodynamic and rolling-resistance forces".into(),
            format!(
                "estimated engine power divides wheel power by (1 - {} drivetrain loss)",
                params.drivetrain_loss
            ),
        ],
    })
}

fn validate_params(params: &VirtualDynoParams) -> Result<(), LogAnalysisError> {
    for (name, value) in [
        ("mass_kg", params.mass_kg),
        ("drag_coefficient", params.drag_coefficient),
        ("frontal_area_m2", params.frontal_area_m2),
        ("rolling_resistance", params.rolling_resistance),
        ("air_density_kg_m3", params.air_density_kg_m3),
    ] {
        if !value.is_finite() || value < 0.0 {
            return Err(LogAnalysisError::InvalidParameter(format!(
                "{name} must be finite and non-negative"
            )));
        }
    }
    if params.mass_kg == 0.0 || params.air_density_kg_m3 == 0.0 {
        return Err(LogAnalysisError::InvalidParameter(
            "mass_kg and air_density_kg_m3 must be positive".into(),
        ));
    }
    if !params.drivetrain_loss.is_finite()
        || params.drivetrain_loss < 0.0
        || params.drivetrain_loss >= 1.0
    {
        return Err(LogAnalysisError::InvalidParameter(
            "drivetrain_loss must be in [0, 1)".into(),
        ));
    }
    if params.smoothing_window == 0 {
        return Err(LogAnalysisError::InvalidParameter(
            "smoothing_window must be at least one".into(),
        ));
    }
    Ok(())
}

fn trailing_mean(values: &[f64], index: usize, window: usize) -> f64 {
    let start = index.saturating_add(1).saturating_sub(window);
    let slice = &values[start..=index];
    if slice.iter().any(|value| !value.is_finite()) {
        f64::NAN
    } else {
        slice.iter().sum::<f64>() / slice.len() as f64
    }
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> VirtualDynoParams {
        VirtualDynoParams {
            speed_channel: "speed".into(),
            rpm_channel: "rpm".into(),
            mass_kg: 1000.0,
            drag_coefficient: 0.0,
            frontal_area_m2: 2.0,
            rolling_resistance: 0.0,
            drivetrain_loss: 0.0,
            smoothing_window: 1,
            air_density_kg_m3: 1.225,
        }
    }

    #[test]
    fn constant_acceleration_matches_newtonian_power() {
        let samples = SampleSet {
            columns: vec!["speed".into(), "rpm".into()],
            t_ms: vec![0.0, 1000.0, 2000.0],
            rows: vec![vec![10.0, 3000.0], vec![12.0, 3600.0], vec![14.0, 4200.0]],
        };
        let report = virtual_dyno(&samples, &params()).unwrap();
        assert_eq!(report.points.len(), 2);
        // F=m*a=2000 N; P=F*v=24 kW at 12 m/s.
        assert!((report.points[0].wheel_power_w - 24_000.0).abs() < 1e-9);
        assert_eq!(report, virtual_dyno(&samples, &params()).unwrap());
    }

    #[test]
    fn drag_and_rolling_losses_increase_required_power() {
        let samples = SampleSet {
            columns: vec!["speed".into(), "rpm".into()],
            t_ms: vec![0.0, 1000.0],
            rows: vec![vec![10.0, 3000.0], vec![11.0, 3300.0]],
        };
        let baseline = virtual_dyno(&samples, &params()).unwrap().points[0].wheel_power_w;
        let mut lossy = params();
        lossy.drag_coefficient = 0.3;
        lossy.rolling_resistance = 0.015;
        let with_losses = virtual_dyno(&samples, &lossy).unwrap().points[0].wheel_power_w;
        assert!(with_losses > baseline);
    }

    #[test]
    fn rejects_bad_time_and_zero_smoothing() {
        let samples = SampleSet {
            columns: vec!["speed".into(), "rpm".into()],
            t_ms: vec![0.0, 0.0],
            rows: vec![vec![1.0, 1000.0], vec![2.0, 2000.0]],
        };
        let report = virtual_dyno(&samples, &params()).unwrap();
        assert!(report.points.is_empty());
        assert!(!report.conditions[1].accepted);
        let mut invalid = params();
        invalid.smoothing_window = 0;
        assert!(matches!(
            virtual_dyno(&samples, &invalid),
            Err(LogAnalysisError::InvalidParameter(_))
        ));
    }
}
