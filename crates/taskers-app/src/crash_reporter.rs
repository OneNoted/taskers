use std::{
    any::Any,
    backtrace::Backtrace,
    fs,
    path::{Path, PathBuf},
    process,
    sync::Arc,
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

const REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone)]
pub struct CrashReporter {
    paths: Arc<CrashReporterPaths>,
}

#[derive(Clone)]
struct CrashReporterPaths {
    session_path: PathBuf,
    config_path: PathBuf,
    ui_integrity_path: PathBuf,
    run_marker_path: PathBuf,
    latest_report_path: PathBuf,
    archive_dir: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RunMarker {
    schema_version: u32,
    pid: u32,
    started_at: String,
    executable: String,
    session_path: String,
    config_path: String,
    ui_integrity_path: String,
    version: String,
}

#[derive(Debug, Serialize)]
struct CrashReport {
    schema_version: u32,
    kind: &'static str,
    recorded_at: String,
    current_pid: u32,
    current_executable: String,
    previous_run: Option<RunMarker>,
    suggested_coredumpctl: Option<String>,
    ui_integrity_path: String,
    ui_integrity_exists: bool,
    session_path: String,
    session_exists: bool,
    panic: Option<PanicDetails>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct PanicDetails {
    thread_name: Option<String>,
    location: Option<String>,
    message: String,
    backtrace: String,
}

impl CrashReporter {
    pub fn for_session(session_path: &Path, config_path: &Path) -> Self {
        let state_dir = session_path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        let ui_integrity_path = std::env::var_os("TASKERS_UI_INTEGRITY_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| state_dir.join("ui-integrity.json"));
        let archive_dir = state_dir.join("crashes");
        let latest_report_path = archive_dir.join("latest.json");
        let run_marker_path = state_dir.join("run-marker.json");

        Self {
            paths: Arc::new(CrashReporterPaths {
                session_path: session_path.to_path_buf(),
                config_path: config_path.to_path_buf(),
                ui_integrity_path,
                run_marker_path,
                latest_report_path,
                archive_dir,
            }),
        }
    }

    pub fn ui_integrity_path(&self) -> &Path {
        &self.paths.ui_integrity_path
    }

    pub fn install_panic_hook(&self) {
        let previous_hook = std::panic::take_hook();
        let reporter = self.clone();
        std::panic::set_hook(Box::new(move |info| {
            if let Err(error) = reporter.write_panic_report(info) {
                eprintln!("failed to write taskers panic report: {error}");
            }
            previous_hook(info);
        }));
    }

    pub fn recover_previous_run(&self) -> Result<Option<PathBuf>> {
        if !self.paths.run_marker_path.exists() {
            return Ok(None);
        }

        let marker_text = fs::read_to_string(&self.paths.run_marker_path).with_context(|| {
            format!(
                "failed to read run marker {}",
                self.paths.run_marker_path.display()
            )
        })?;
        let (previous_run, notes) = match serde_json::from_str::<RunMarker>(&marker_text) {
            Ok(marker) => (
                Some(marker.clone()),
                vec!["detected an unclean shutdown from the previous run".to_string()],
            ),
            Err(error) => (
                None,
                vec![format!("run marker could not be parsed: {error}")],
            ),
        };

        let report = CrashReport {
            schema_version: REPORT_SCHEMA_VERSION,
            kind: "unclean_shutdown",
            recorded_at: now_rfc3339(),
            current_pid: process::id(),
            current_executable: current_executable(),
            suggested_coredumpctl: previous_run
                .as_ref()
                .map(|marker| format!("coredumpctl info {}", marker.pid)),
            ui_integrity_path: previous_run
                .as_ref()
                .map(|marker| marker.ui_integrity_path.clone())
                .unwrap_or_else(|| self.paths.ui_integrity_path.display().to_string()),
            ui_integrity_exists: self.paths.ui_integrity_path.exists(),
            session_path: previous_run
                .as_ref()
                .map(|marker| marker.session_path.clone())
                .unwrap_or_else(|| self.paths.session_path.display().to_string()),
            session_exists: self.paths.session_path.exists(),
            previous_run,
            panic: None,
            notes,
        };

        let report_path = self.write_report("unclean_shutdown", &report)?;
        let _ = fs::remove_file(&self.paths.run_marker_path);
        Ok(Some(report_path))
    }

    pub fn mark_launch(&self) -> Result<()> {
        let marker = RunMarker {
            schema_version: REPORT_SCHEMA_VERSION,
            pid: process::id(),
            started_at: now_rfc3339(),
            executable: current_executable(),
            session_path: self.paths.session_path.display().to_string(),
            config_path: self.paths.config_path.display().to_string(),
            ui_integrity_path: self.paths.ui_integrity_path.display().to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        };
        write_json_atomic(&self.paths.run_marker_path, &marker)
    }

    pub fn mark_clean_shutdown(&self) -> Result<()> {
        if self.paths.run_marker_path.exists() {
            fs::remove_file(&self.paths.run_marker_path).with_context(|| {
                format!(
                    "failed to remove run marker {}",
                    self.paths.run_marker_path.display()
                )
            })?;
        }
        Ok(())
    }

    fn write_panic_report(&self, info: &std::panic::PanicHookInfo<'_>) -> Result<PathBuf> {
        let message = panic_message(info.payload());
        let location = info.location().map(|location| {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        });
        let report = CrashReport {
            schema_version: REPORT_SCHEMA_VERSION,
            kind: "panic",
            recorded_at: now_rfc3339(),
            current_pid: process::id(),
            current_executable: current_executable(),
            suggested_coredumpctl: Some(format!("coredumpctl info {}", process::id())),
            ui_integrity_path: self.paths.ui_integrity_path.display().to_string(),
            ui_integrity_exists: self.paths.ui_integrity_path.exists(),
            session_path: self.paths.session_path.display().to_string(),
            session_exists: self.paths.session_path.exists(),
            previous_run: self.read_run_marker().ok().flatten(),
            panic: Some(PanicDetails {
                thread_name: std::thread::current().name().map(ToOwned::to_owned),
                location,
                message,
                backtrace: Backtrace::force_capture().to_string(),
            }),
            notes: vec!["panic hook captured a Rust panic before shutdown".to_string()],
        };
        self.write_report("panic", &report)
    }

    fn write_report(&self, kind: &str, report: &CrashReport) -> Result<PathBuf> {
        let timestamp = OffsetDateTime::now_utc().unix_timestamp();
        let archive_path = self
            .paths
            .archive_dir
            .join(format!("crash-{timestamp}-{kind}.json"));
        write_json_atomic(&self.paths.latest_report_path, report)?;
        write_json_atomic(&archive_path, report)?;
        Ok(archive_path)
    }

    fn read_run_marker(&self) -> Result<Option<RunMarker>> {
        if !self.paths.run_marker_path.exists() {
            return Ok(None);
        }
        let marker = fs::read_to_string(&self.paths.run_marker_path).with_context(|| {
            format!(
                "failed to read run marker {}",
                self.paths.run_marker_path.display()
            )
        })?;
        Ok(Some(serde_json::from_str(&marker)?))
    }
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }
    let encoded = serde_json::to_vec_pretty(value)?;
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, encoded)
        .with_context(|| format!("failed to write {}", tmp_path.display()))?;
    fs::rename(&tmp_path, path).with_context(|| format!("failed to publish {}", path.display()))?;
    Ok(())
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| OffsetDateTime::now_utc().unix_timestamp().to_string())
}

fn current_executable() -> String {
    std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|error| format!("unresolved executable: {error}"))
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::CrashReporter;
    use serde_json::Value;
    use tempfile::tempdir;

    #[test]
    fn recovers_unclean_shutdown_into_report_file() {
        let tempdir = tempdir().expect("tempdir");
        let session_path = tempdir.path().join("session.json");
        let config_path = tempdir.path().join("config.json");
        let reporter = CrashReporter::for_session(&session_path, &config_path);

        reporter.mark_launch().expect("launch marker");
        let report_path = reporter
            .recover_previous_run()
            .expect("recovery should succeed")
            .expect("report path");

        assert!(!tempdir.path().join("run-marker.json").exists());
        assert!(report_path.exists());
        assert!(tempdir.path().join("crashes").join("latest.json").exists());

        let latest: Value = serde_json::from_str(
            &std::fs::read_to_string(tempdir.path().join("crashes").join("latest.json"))
                .expect("latest report"),
        )
        .expect("valid latest report");
        assert_eq!(latest["kind"], "unclean_shutdown");
    }

    #[test]
    fn clean_shutdown_removes_run_marker() {
        let tempdir = tempdir().expect("tempdir");
        let session_path = tempdir.path().join("session.json");
        let config_path = tempdir.path().join("config.json");
        let reporter = CrashReporter::for_session(&session_path, &config_path);

        reporter.mark_launch().expect("launch marker");
        reporter.mark_clean_shutdown().expect("clean shutdown");

        assert!(!tempdir.path().join("run-marker.json").exists());
    }
}
