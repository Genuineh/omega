use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex};

use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

pub struct TracingConfig {
    pub env_filter: Option<String>,
    pub log_dir: Option<PathBuf>,
    pub enable_file_log: bool,
    pub human_readable_sink: Option<mpsc::SyncSender<String>>,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            env_filter: None,
            log_dir: None,
            enable_file_log: true,
            human_readable_sink: None,
        }
    }
}

pub struct UiWriter(Option<mpsc::SyncSender<String>>);

pub fn init_tracing_channel() -> anyhow::Result<mpsc::Receiver<String>> {
    let (trace_tx, trace_rx) = mpsc::sync_channel::<String>(1024);
    init_tracing(TracingConfig {
        human_readable_sink: Some(trace_tx),
        ..TracingConfig::default()
    })?;
    Ok(trace_rx)
}

impl Write for UiWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        for line in text.lines() {
            if !line.is_empty() {
                if let Some(tx) = &self.0 {
                    let _ = tx.send(line.to_string());
                }
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub fn init_tracing(config: TracingConfig) -> anyhow::Result<()> {
    let env_filter = match config.env_filter {
        Some(value) => EnvFilter::new(value),
        None => EnvFilter::try_from_env("OMEGA_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
    };

    let human_layer = config.human_readable_sink.map(|sink| {
        tracing_subscriber::fmt::layer()
            .compact()
            .with_ansi(false)
            .with_target(true)
            .with_thread_ids(false)
            .with_span_events(FmtSpan::CLOSE)
            .with_writer(Mutex::new(UiWriter(Some(sink))))
    });

    let file_layer =
        create_log_file(config.log_dir.as_deref(), config.enable_file_log)?.map(|file| {
            tracing_subscriber::fmt::layer()
                .json()
                .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
                .with_writer(Mutex::new(file))
        });

    tracing_subscriber::registry()
        .with(env_filter)
        .with(human_layer)
        .with(file_layer)
        .try_init()
        .map_err(|e| anyhow::anyhow!("failed to initialize tracing: {e}"))?;

    Ok(())
}

fn create_log_file(
    log_dir: Option<&Path>,
    enable_file_log: bool,
) -> anyhow::Result<Option<std::fs::File>> {
    if !file_logging_enabled(
        enable_file_log,
        std::env::var("OMEGA_LOG_FILE").ok().as_deref(),
    ) {
        return Ok(None);
    }

    let log_dir = resolve_log_dir(log_dir.map(Path::to_path_buf))?;
    std::fs::create_dir_all(&log_dir)?;

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let path = build_log_path(&log_dir, &today);

    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;

    Ok(Some(file))
}

fn resolve_log_dir(log_dir: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    match log_dir {
        Some(path) => Ok(path),
        None => {
            let home = dirs::home_dir()
                .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
            Ok(home.join(".omega").join("logs"))
        }
    }
}

fn build_log_path(log_dir: &Path, today: &str) -> PathBuf {
    log_dir.join(format!("omega-{today}.jsonl"))
}

fn file_logging_enabled(enable_file_log: bool, env_override: Option<&str>) -> bool {
    enable_file_log
        && env_override
            .map(|value| value != "false" && value != "0")
            .unwrap_or(true)
}

pub fn strip_ansi(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == 0x1b {
            if index + 1 < bytes.len() && bytes[index + 1] == b'[' {
                index += 2;
                while index < bytes.len() && !bytes[index].is_ascii_alphabetic() {
                    index += 1;
                }
                if index < bytes.len() {
                    index += 1;
                }
            } else if index + 1 < bytes.len() && bytes[index + 1] == b']' {
                index += 2;
                while index < bytes.len() {
                    if bytes[index] == 0x07 {
                        index += 1;
                        break;
                    }
                    if bytes[index] == 0x1b && index + 1 < bytes.len() && bytes[index + 1] == b'\\'
                    {
                        index += 2;
                        break;
                    }
                    index += 1;
                }
            } else {
                index += 2;
            }
        } else {
            result.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&result).into_owned()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{build_log_path, file_logging_enabled, resolve_log_dir, strip_ansi};

    #[test]
    fn strips_csi_escape_sequences() {
        assert_eq!(strip_ansi("\u{1b}[31mhello\u{1b}[0m"), "hello");
    }

    #[test]
    fn explicit_log_dir_is_preserved() {
        let path = PathBuf::from("/tmp/omega-logs");
        assert_eq!(resolve_log_dir(Some(path.clone())).unwrap(), path);
    }

    #[test]
    fn file_logging_respects_disable_flags() {
        assert!(!file_logging_enabled(false, None));
        assert!(!file_logging_enabled(true, Some("false")));
        assert!(!file_logging_enabled(true, Some("0")));
        assert!(file_logging_enabled(true, Some("true")));
    }

    #[test]
    fn log_path_uses_daily_jsonl_name() {
        let path = build_log_path(PathBuf::from("/tmp").as_path(), "2026-03-19");
        assert_eq!(path, PathBuf::from("/tmp/omega-2026-03-19.jsonl"));
    }
}
