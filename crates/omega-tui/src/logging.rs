use std::io::{self, Write};
use std::sync::{mpsc, Mutex};

use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

pub struct UiWriter(pub Mutex<Option<mpsc::SyncSender<String>>>);

pub fn init_tracing_channel() -> anyhow::Result<mpsc::Receiver<String>> {
    let (trace_tx, trace_rx) = mpsc::sync_channel::<String>(1024);
    init_tracing(Mutex::new(Some(trace_tx)))?;
    Ok(trace_rx)
}

impl Write for UiWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        for line in text.lines() {
            if !line.is_empty() {
                if let Some(tx) = self.0.lock().unwrap().as_ref() {
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

pub fn init_tracing(tx: Mutex<Option<mpsc::SyncSender<String>>>) -> anyhow::Result<()> {
    let env_filter =
        EnvFilter::try_from_env("OMEGA_LOG").unwrap_or_else(|_| EnvFilter::new("info"));

    let ui_layer = tracing_subscriber::fmt::layer()
        .compact()
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(false)
        .with_span_events(FmtSpan::CLOSE)
        .with_writer(Mutex::new(UiWriter(tx)));

    let file_layer = match create_log_file() {
        Ok(file) => {
            let writer = Mutex::new(file);
            let layer = tracing_subscriber::fmt::layer()
                .json()
                .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
                .with_writer(writer);
            Some(layer)
        }
        Err(e) => {
            eprintln!("warn: failed to create log file, file logging disabled: {e}");
            None
        }
    };

    tracing_subscriber::registry()
        .with(env_filter)
        .with(ui_layer)
        .with(file_layer)
        .try_init()
        .map_err(|e| anyhow::anyhow!("failed to initialize tracing: {e}"))?;

    Ok(())
}

fn create_log_file() -> anyhow::Result<std::fs::File> {
    let log_dir = match std::env::var("OMEGA_LOG_DIR") {
        Ok(dir) => std::path::PathBuf::from(dir),
        Err(_) => {
            let home = dirs::home_dir()
                .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
            home.join(".omega").join("logs")
        }
    };

    let enabled = std::env::var("OMEGA_LOG_FILE")
        .map(|value| value != "false" && value != "0")
        .unwrap_or(true);
    if !enabled {
        return Err(anyhow::anyhow!("file logging disabled by OMEGA_LOG_FILE"));
    }

    std::fs::create_dir_all(&log_dir)?;

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let path = log_dir.join(format!("omega-{today}.jsonl"));

    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;

    Ok(file)
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
    use super::strip_ansi;

    #[test]
    fn strips_csi_escape_sequences() {
        assert_eq!(strip_ansi("\u{1b}[31mhello\u{1b}[0m"), "hello");
    }
}
