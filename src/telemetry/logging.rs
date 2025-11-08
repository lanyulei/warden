//! Logging module with async file rotation and multi-output support.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};

use anyhow::{anyhow, Result};
use tracing_subscriber::{
    filter::EnvFilter,
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    reload,
    util::SubscriberInitExt,
    Registry,
    Layer, // For .boxed()
};
use once_cell::sync::OnceCell;

use crate::config::schema::TelemetryConfig;

/// Global logger handle for background thread management
static LOGGER_HANDLE: OnceCell<LoggerHandle> = OnceCell::new();

// Macro for base fmt::layer construction (no format/writer)
macro_rules! base_fmt_layer {
    () => {
        fmt::layer()
            .with_ansi(false)
            .with_file(true)
            .with_line_number(true)
            .with_target(true)
            .with_level(true)
            .with_thread_ids(false)
            .with_thread_names(false)
            .with_span_events(FmtSpan::NONE)
    };
}

/// Async file write commands
enum Cmd {
    Write(Vec<u8>),
    Flush,
    Shutdown,
}

/// Rotating file writer with size-based rotation and retention
struct RotatingFileWorker {
    base_path: PathBuf,
    file: Option<File>,
    current_size: u64,
    max_size: u64,
    keep: usize,    // Number of rotated files to keep
    compress: bool, // Compress rotated files (gzip)
}

impl RotatingFileWorker {
    fn new<P: AsRef<Path>>(base: P, max_size: u64, keep: usize, compress: bool) -> io::Result<Self> {
        let base_path = base.as_ref().to_path_buf();
        if let Some(dir) = base_path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut worker = Self {
            base_path,
            file: None,
            current_size: 0,
            max_size,
            keep,
            compress,
        };
        worker.open_new_file()?;
        Ok(worker)
    }

    fn open_new_file(&mut self) -> io::Result<()> {
        let f = OpenOptions::new()
            .create(true)
            .append(true)
            .write(true)
            .open(&self.base_path)?;
        let size = f.metadata().map(|m| m.len()).unwrap_or(0);
        self.file = Some(f);
        self.current_size = size;
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.file.take();
        if self.keep > 0 {
            for i in (1..=self.keep).rev() {
                let src = self.suffixed(i);
                let dst = self.suffixed(i + 1);
                if src.exists() {
                    if i == self.keep && dst.exists() {
                        let _ = fs::remove_file(&dst);
                    }
                    let _ = fs::rename(&src, &dst);
                }
            }
            if self.base_path.exists() {
                let dst = self.suffixed(1);
                if dst.exists() {
                    let _ = fs::remove_file(&dst);
                }
                let _ = fs::rename(&self.base_path, &dst);
                if self.compress {
                    let _ = Self::compress_file(&dst);
                }
            }
        } else {
            if self.base_path.exists() {
                let _ = fs::remove_file(&self.base_path);
            }
        }
        self.open_new_file()
    }

    #[inline]
    fn suffixed(&self, n: usize) -> PathBuf {
        let mut p = self.base_path.clone();
        let name = p
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| format!("{}.{}", s, n))
            .unwrap_or_else(|| format!(".{}", n));
        p.set_file_name(name);
        p
    }

    fn write(&mut self, buf: &[u8]) -> io::Result<()> {
        if self.current_size + (buf.len() as u64) > self.max_size {
            self.rotate()?;
        }
        if let Some(f) = self.file.as_mut() {
            f.write_all(buf)?;
            self.current_size += buf.len() as u64;
        }
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(f) = self.file.as_mut() {
            f.flush()?;
        }
        Ok(())
    }

    /// Placeholder for gzip compression (can be enhanced with flate2)
    #[allow(unused)]
    fn compress_file(_path: &Path) -> io::Result<()> {
        Ok(())
    }
}

/// MultiWriter: unified stdout/file/both output
#[derive(Clone)]
struct MultiWriter {
    to_stdout: bool,
    file_tx: Option<mpsc::Sender<Cmd>>,
}

struct MultiWriterHandle {
    to_stdout: bool,
    file_tx: Option<mpsc::Sender<Cmd>>,
}

impl Write for MultiWriterHandle {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.to_stdout {
            let _ = std::io::stdout().write_all(buf);
        }
        if let Some(tx) = &self.file_tx {
            let _ = tx.send(Cmd::Write(buf.to_vec()));
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.to_stdout {
            let _ = std::io::stdout().flush();
        }
        if let Some(tx) = &self.file_tx {
            let _ = tx.send(Cmd::Flush);
        }
        Ok(())
    }
}

impl<'a> fmt::MakeWriter<'a> for MultiWriter {
    type Writer = MultiWriterHandle;
    fn make_writer(&'a self) -> Self::Writer {
        MultiWriterHandle {
            to_stdout: self.to_stdout,
            file_tx: self.file_tx.clone(),
        }
    }
}

/// Logger handle for graceful shutdown and dynamic level
pub struct LoggerHandle {
    _bg: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
    tx: mpsc::Sender<Cmd>,
}

impl Drop for LoggerHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Shutdown);
        if let Ok(mut g) = self._bg.lock() {
            if let Some(h) = g.take() {
                let _ = h.join();
            }
        }
    }
}

/// Validate TelemetryConfig logging fields
fn validate_config(cfg: &TelemetryConfig) -> Result<()> {
    match cfg.log_level.to_ascii_lowercase().as_str() {
        "error" | "warn" | "info" | "debug" | "trace" => {}
        other => return Err(anyhow!("invalid log_level: {}", other)),
    }
    match cfg.log_format.to_ascii_lowercase().as_str() {
        "json" | "plain" => {}
        other => return Err(anyhow!("invalid log_format: {}", other)),
    }
    match cfg.log_output.to_ascii_lowercase().as_str() {
        "stdout" | "file" | "both" => {}
        other => return Err(anyhow!("invalid log_output: {}", other)),
    }
    if (cfg.log_output == "file" || cfg.log_output == "both") && cfg.log_file.trim().is_empty() {
        return Err(anyhow!("log_file required when output=file/both"));
    }
    if cfg.log_rotation.max_size_mb == 0 {
        return Err(anyhow!("max_size_mb must be > 0"));
    }
    if cfg.log_rotation.max_files == 0 {
        return Err(anyhow!("max_files must be > 0"));
    }
    Ok(())
}

/// Initialize global logger (call early in main)
/// - Supports JSON/plain format
/// - Output: stdout/file/both
/// - Async file write with rotation
pub fn init_logging(cfg: &TelemetryConfig) -> Result<LoggerHandle> {
    validate_config(cfg)?;
    let default_level = cfg.log_level.to_ascii_lowercase();
    let filter = EnvFilter::try_new(default_level.clone()).unwrap_or_else(|_| EnvFilter::new("info"));
    let (filter_layer, _filter_handle) = reload::Layer::new(filter);

    let keep = if cfg.log_rotation.max_files > 0 { (cfg.log_rotation.max_files - 1) as usize } else { 0 };
    let max_size_bytes = (cfg.log_rotation.max_size_mb as u64) * 1024 * 1024;
    let (file_tx, bg_handle_opt) = if cfg.log_output == "file" || cfg.log_output == "both" {
        let (tx, rx) = mpsc::channel::<Cmd>();
        let base = PathBuf::from(cfg.log_file.clone());
        let compress = cfg.log_rotation.compress;
        let bg = thread::Builder::new().name("log-rotate-writer".into()).spawn(move || {
            let mut worker = match RotatingFileWorker::new(&base, max_size_bytes, keep, compress) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("[logging] failed to init file writer: {e}");
                    return;
                }
            };
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    Cmd::Write(buf) => {
                        if let Err(e) = worker.write(&buf) {
                            eprintln!("[logging] write error: {e}");
                            thread::sleep(Duration::from_millis(5));
                        }
                    }
                    Cmd::Flush => { let _ = worker.flush(); }
                    Cmd::Shutdown => {
                        let _ = worker.flush();
                        break;
                    }
                }
            }
        }).expect("spawn log writer thread");
        (Some(tx), Some(bg))
    } else { (None, None) };

    let multi_writer = MultiWriter {
        to_stdout: cfg.log_output == "stdout" || cfg.log_output == "both",
        file_tx: file_tx.clone(),
    };

    let fmt_layer = if cfg.log_format == "json" {
        base_fmt_layer!()
            .json()
            .flatten_event(true)
            .with_current_span(true)
            .with_span_list(true)
            .with_writer(multi_writer.clone())
            .boxed()
    } else {
        base_fmt_layer!()
            .compact()
            .with_writer(multi_writer.clone())
            .boxed()
    };

    Registry::default()
        .with(filter_layer)
        .with(fmt_layer)
        .try_init()
        .ok();

    let bg = Arc::new(Mutex::new(bg_handle_opt));
    let tx = file_tx.unwrap_or_else(|| {
        let (tx, _rx) = mpsc::channel();
        tx
    });
    Ok(LoggerHandle { _bg: bg, tx })
}

/// Initialize global logger and save handle (error if already set)
pub fn init_global_logging(cfg: &TelemetryConfig) -> Result<&'static LoggerHandle> {
    let handle = init_logging(cfg)?;
    LOGGER_HANDLE
        .set(handle)
        .map_err(|_| anyhow!("Logger already initialized"))?;
    Ok(LOGGER_HANDLE.get().expect("logger set"))
}
