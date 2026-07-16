// SPDX-License-Identifier: GPL-3.0-or-later
//! Datalog recording/playback and post-hoc analysis command handling for the
//! §9 owner task. Split from `owner.rs` for file cohesion: everything here
//! is the `start_log`/`stop_log`/`open_log`/`save_log`/`get_log_data` family
//! plus the log-stats/anomaly/virtual-dyno analysis commands, operating on
//! [`super::Owner`]'s private `active_log`/`opened_log` state the same as
//! everything left in the parent.

use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use opentune_datalog::{Field, Log, LogEntry, Marker, Record};
use opentune_realtime::RealtimePoller;

use super::Owner;
use crate::dto::{
    AnomalyReportDto, AnomalyThresholdsDto, LogFormatDto, LogStatsParamsDto, LogStatsReportDto,
    LogStatusDto, LogSummaryDto, VirtualDynoParamsDto, VirtualDynoReportDto,
};
use crate::log_paths::{validate_log_read_path, validate_log_write_path};
use crate::session::NO_OCH_BLOCK;

/// M5 review H1/M1: reject an `open_log` source larger than this before
/// reading it into memory — `opentune_datalog::read_log` materializes the
/// whole file (both the CSV and MLG readers `read_to_string`/`read_to_end`),
/// so an unbounded read of a huge or corrupt file could exhaust memory.
pub(super) const MAX_LOG_FILE_BYTES: u64 = 256 * 1024 * 1024;
/// M5 review CRITICAL (C2): a command targeting `opened_log` with a
/// `log_id` that no longer matches [`Owner::log_generation`] gets this
/// instead of silently reading whatever log now occupies the slot — see
/// [`Owner::check_log_id`].
const LOG_CHANGED: &str = "log changed since it was opened";
/// [`Owner::stop_log`] (and [`Owner::add_log_marker`]) report this when there
/// is no [`Owner::active_log`] to act on. `pub(crate)` so the exit-flush path
/// (M5 review CRITICAL C3, `lib.rs`) can tell "nothing to flush" apart from a
/// real flush failure without duplicating the literal.
pub(crate) const NO_ACTIVE_LOG: &str = "no active log";

pub(super) struct ActiveLog {
    /// The raw path the webview asked to record to, kept verbatim for the
    /// status UI (`log_status`).
    pub(super) path: String,
    /// The validated write target resolved once at `start_log`, so `stop_log`
    /// writes to the destination chosen up front instead of re-resolving a
    /// path whose meaning may have shifted mid-recording.
    validated: PathBuf,
    pub(super) format: LogFormatDto,
    pub(super) log: Log,
    started: Instant,
    counter: u8,
}

impl Owner {
    pub(super) fn start_log(
        &mut self,
        path: String,
        format: LogFormatDto,
    ) -> Result<LogStatusDto, String> {
        if self.active_log.is_some() {
            return Err("a log is already active".into());
        }
        let validated = validate_log_write_path(&path)?;
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| self.not_connected_error())?;
        // M5 review M2 + och>u16 guard: mirror `Session::poll_frame_full`'s
        // own frame-length gate up front. An och block size of zero means no
        // realtime frame will ever feed this log; one above `u16::MAX` can't
        // be requested as a read length at all. Either way every poll would
        // fail and the log would silently record zero rows for the whole
        // session, so reject both here instead of after the fact.
        let och_len = u16::try_from(session.def.comms.och_block_size).map_err(|_| {
            format!(
                "cannot start log: ochBlockSize {} exceeds u16",
                session.def.comms.och_block_size
            )
        })?;
        if och_len == 0 {
            return Err(format!("cannot start log: {NO_OCH_BLOCK}"));
        }
        let fields = session
            .def
            .output_channels
            .iter()
            .map(|channel| Field::float(channel.name(), ""))
            .collect();
        let mut log = Log::new(fields);
        log.started_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| {
                u32::try_from(duration.as_secs()).unwrap_or(u32::MAX)
            });
        self.active_log = Some(ActiveLog {
            path,
            validated,
            format,
            log,
            started: Instant::now(),
            counter: 0,
        });
        // Logging requires acquisition, but does not implicitly enable UI
        // realtime events. `wants_poll` keeps the owner ticking for the log.
        self.poller.get_or_insert_with(RealtimePoller::default);
        Ok(self.log_status())
    }

    pub(super) async fn stop_log(&mut self) -> Result<LogSummaryDto, String> {
        let active = self
            .active_log
            .take()
            .ok_or_else(|| NO_ACTIVE_LOG.to_string())?;
        // Write to the target resolved at `start_log`, not by re-validating
        // `active.path` — the destination was chosen when recording began.
        let target = active.validated;
        let format = active.format;
        let log = active.log;
        let result = tokio::task::spawn_blocking(move || {
            let result = write_validated(&target, format, &log);
            (log, result)
        })
        .await
        .map_err(|error| format!("log write panicked: {error}"))?;
        let (log, write_result) = result;
        let log_id = self.assign_opened_log(log);
        let summary = crate::log_bridge::summary(
            self.opened_log
                .as_ref()
                .expect("assign_opened_log just set this"),
            log_id,
        );
        if !self.polling {
            self.poller = None;
        }
        write_result?;
        Ok(summary)
    }

    pub(super) fn add_log_marker(&mut self, text: String) -> Result<(), String> {
        if !text.is_ascii() || text.len() > 49 {
            return Err("MLG v1 marker text must be ASCII and at most 49 bytes".into());
        }
        let active = self
            .active_log
            .as_mut()
            .ok_or_else(|| NO_ACTIVE_LOG.to_string())?;
        let timestamp_10us = active.timestamp();
        active.log.entries.push(LogEntry::Marker(Marker {
            counter: active.counter,
            timestamp_10us,
            text,
        }));
        active.counter = next_counter(active.counter);
        Ok(())
    }

    pub(super) fn log_status(&self) -> LogStatusDto {
        match &self.active_log {
            Some(active) => LogStatusDto {
                active: true,
                path: Some(active.path.clone()),
                format: Some(active.format),
                record_count: saturating_u32(active.log.records().count()),
            },
            None => LogStatusDto {
                active: false,
                path: None,
                format: None,
                record_count: 0,
            },
        }
    }

    pub(super) async fn open_log(
        &mut self,
        path: String,
        format: LogFormatDto,
    ) -> Result<LogSummaryDto, String> {
        let log = tokio::task::spawn_blocking(move || read_log_path(&path, format))
            .await
            .map_err(|error| format!("log read panicked: {error}"))??;
        let log_id = self.assign_opened_log(log);
        Ok(crate::log_bridge::summary(
            self.opened_log
                .as_ref()
                .expect("assign_opened_log just set this"),
            log_id,
        ))
    }

    pub(super) async fn save_log(
        &mut self,
        log_id: u32,
        path: String,
        format: LogFormatDto,
    ) -> Result<(), String> {
        self.check_log_id(log_id)?;
        let log = self
            .opened_log
            .clone()
            .ok_or_else(|| "no log opened".to_string())?;
        tokio::task::spawn_blocking(move || write_log_path(&path, format, &log))
            .await
            .map_err(|error| format!("log write panicked: {error}"))?
    }

    pub(super) async fn run_log_stats(
        &mut self,
        log_id: u32,
        params: LogStatsParamsDto,
    ) -> Result<LogStatsReportDto, String> {
        let samples = self.opened_samples(log_id)?;
        tokio::task::spawn_blocking(move || {
            let params = crate::log_bridge::stats_params(params);
            opentune_analysis::log_stats(&samples, &params)
                .map(crate::log_bridge::stats_report)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| format!("log stats panicked: {error}"))?
    }

    pub(super) async fn run_detect_anomaly(
        &mut self,
        log_id: u32,
        thresholds: AnomalyThresholdsDto,
    ) -> Result<AnomalyReportDto, String> {
        let samples = self.opened_samples(log_id)?;
        tokio::task::spawn_blocking(move || {
            let thresholds = crate::log_bridge::anomaly_params(thresholds);
            opentune_analysis::detect_anomaly(&samples, &thresholds)
                .map(crate::log_bridge::anomaly_report)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| format!("anomaly analysis panicked: {error}"))?
    }

    pub(super) async fn run_virtual_dyno(
        &mut self,
        log_id: u32,
        params: VirtualDynoParamsDto,
    ) -> Result<VirtualDynoReportDto, String> {
        let samples = self.opened_samples(log_id)?;
        tokio::task::spawn_blocking(move || {
            let params = crate::log_bridge::dyno_params(params);
            opentune_analysis::virtual_dyno(&samples, &params)
                .map(crate::log_bridge::dyno_report)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| format!("virtual dyno panicked: {error}"))?
    }

    pub(super) fn opened_samples(
        &self,
        log_id: u32,
    ) -> Result<opentune_analysis::SampleSet, String> {
        self.check_log_id(log_id)?;
        self.opened_log
            .as_ref()
            .map(crate::log_bridge::to_samples)
            .ok_or_else(|| "no log opened".to_string())
    }

    /// The `log_id` guard shared by every command reading `opened_log` (M5
    /// review CRITICAL — C2): rejects a caller holding an id from a
    /// superseded `open_log`/`stop_log` instead of silently answering with
    /// whatever log now occupies the slot.
    pub(super) fn check_log_id(&self, log_id: u32) -> Result<(), String> {
        if log_id == self.log_generation {
            Ok(())
        } else {
            Err(LOG_CHANGED.to_owned())
        }
    }

    /// Assign a freshly read (`open_log`) or just-recorded (`stop_log`) log
    /// as the current `opened_log`, minting a new generation token — the
    /// `log_id` every later `opened_log`-reading command must echo back.
    /// `log_generation` starts at `0`, which this always increments past
    /// first, so `0` never matches a real assignment (see
    /// [`Owner::log_generation`]'s doc).
    pub(super) fn assign_opened_log(&mut self, log: Log) -> u32 {
        self.log_generation = self.log_generation.wrapping_add(1);
        self.opened_log = Some(log);
        self.log_generation
    }
}

impl ActiveLog {
    fn timestamp(&self) -> u64 {
        let ticks = self.started.elapsed().as_micros() / 10;
        u64::try_from(ticks).unwrap_or(u64::MAX)
    }

    pub(super) fn push(&mut self, frame: &opentune_realtime::RealtimeFrame) {
        let values = self
            .log
            .fields
            .iter()
            .map(|field| {
                frame
                    .channels
                    .iter()
                    .find(|channel| channel.name == field.name)
                    .map_or(f64::NAN, |channel| channel.value)
            })
            .collect();
        self.log.entries.push(LogEntry::Record(Record {
            counter: self.counter,
            timestamp_10us: self.timestamp(),
            values,
        }));
        self.counter = next_counter(self.counter);
    }
}

fn next_counter(counter: u8) -> u8 {
    if counter == 254 {
        0
    } else {
        counter + 1
    }
}

pub(super) fn read_log_path(path: &str, format: LogFormatDto) -> Result<Log, String> {
    let validated = validate_log_read_path(path)?;
    let metadata = std::fs::metadata(&validated)
        .map_err(|error| format!("{}: {error}", validated.display()))?;
    if metadata.len() > MAX_LOG_FILE_BYTES {
        return Err(format!(
            "log file `{}` is {} bytes, over the {MAX_LOG_FILE_BYTES}-byte limit",
            validated.display(),
            metadata.len()
        ));
    }
    let file = std::fs::File::open(&validated)
        .map_err(|error| format!("{}: {error}", validated.display()))?;
    opentune_datalog::read_log(std::io::BufReader::new(file), format.into())
        .map_err(|error| error.to_string())
}

/// Write `log` atomically: the whole encode + write happens on a temp file
/// in the same directory as `path`, which is then renamed over the
/// destination — a crash or error mid-write can never leave a truncated
/// destination file (M5 review M5). The temp file is removed on any error.
pub(super) fn write_log_path(path: &str, format: LogFormatDto, log: &Log) -> Result<(), String> {
    let target = validate_log_write_path(path)?;
    write_validated(&target, format, log)
}

/// Atomically write `log` to an already-validated `target` (see
/// [`write_log_path`], which validates a raw webview path first). Splitting
/// the write off the validation lets `stop_log` reuse the target it resolved
/// at `start_log` without re-validating.
pub(super) fn write_validated(
    target: &Path,
    format: LogFormatDto,
    log: &Log,
) -> Result<(), String> {
    let temp_path = temp_write_path(target);

    let result = (|| -> Result<(), String> {
        let file = std::fs::File::create(&temp_path)
            .map_err(|error| format!("{}: {error}", temp_path.display()))?;
        let mut writer = std::io::BufWriter::new(file);
        opentune_datalog::write_log(log, &mut writer, format.into())
            .map_err(|error| error.to_string())?;
        use std::io::Write as _;
        writer.flush().map_err(|error| error.to_string())?;
        drop(writer);
        std::fs::rename(&temp_path, target)
            .map_err(|error| format!("{}: {error}", target.display()))
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

/// `<name>.<ext>.tmp-<pid>` in the same directory as `target`, so the final
/// `rename` stays on one filesystem (atomic) rather than becoming a
/// cross-device copy.
fn temp_write_path(target: &std::path::Path) -> std::path::PathBuf {
    let mut temp_name = target
        .file_name()
        .map_or_else(|| std::ffi::OsString::from("log"), ToOwned::to_owned);
    temp_name.push(format!(".tmp-{}", std::process::id()));
    target.with_file_name(temp_name)
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
