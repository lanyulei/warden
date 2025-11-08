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
    Layer, // 引入 Layer trait 以支持 .boxed()
};
use once_cell::sync::OnceCell;

use crate::config::schema::TelemetryConfig; // 使用 TelemetryConfig 而不是不存在的 LoggingConfig

// 公共的 fmt::layer 基础构建宏（不包含格式与 writer），用于减少 JSON / plain 分支重复。
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

/// 异步写入命令
enum Cmd {
    Write(Vec<u8>),
    Flush,
    Shutdown,
}

/// 后台文件写入器（带大小轮转与保留）
struct RotatingFileWorker {
    base_path: PathBuf,
    file: Option<File>,
    current_size: u64,
    max_size: u64,
    keep: usize, // 保留的历史文件数量（不含当前 active 文件）
    compress: bool, // 是否对轮转后的旧文件进行压缩（gzip）
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
        // 打开时同步文件大小（可能已有历史内容）
        let size = f.metadata().map(|m| m.len()).unwrap_or(0);
        self.file = Some(f);
        self.current_size = size;
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        // 关闭当前文件
        self.file.take();
        // 依次上移 .keep -> .keep+1，...，.1 -> .2
        if self.keep > 0 {
            for i in (1..=self.keep).rev() {
                let src = self.suffixed(i);
                let dst = self.suffixed(i + 1);
                if src.exists() {
                    // 超过保留则先删除最高位，避免 rename 冲突
                    if i == self.keep && dst.exists() {
                        let _ = fs::remove_file(&dst);
                    }
                    let _ = fs::rename(&src, &dst);
                }
            }
            // base -> .1
            if self.base_path.exists() {
                let dst = self.suffixed(1);
                // 若 .1 存在，先删
                if dst.exists() {
                    let _ = fs::remove_file(&dst);
                }
                let _ = fs::rename(&self.base_path, &dst);
                // 根据配置压缩刚轮转出的文件（.1）
                if self.compress {
                    let _ = Self::compress_file(&dst);
                }
            }
        } else {
            // keep == 0：直接丢弃历史，生成新文件
            if self.base_path.exists() {
                let _ = fs::remove_file(&self.base_path);
            }
        }
        // 重新打开一个空文件
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

    /// 简单 gzip 压缩（如果 flate2 不可用，可后续增强）。当前实现占位：直接返回 Ok(())。
    #[allow(unused)]
    fn compress_file(path: &Path) -> io::Result<()> {
        // 预留：为了不引入额外依赖，当前不做真实压缩，可后续添加 flate2、gzip 支持。
        // 真实实现时可将原文件读取并写入 path.gz，然后删除原文件。
        Ok(())
    }
}

/// MultiWriter：统一 stdout / file / both 输出，避免多层类型不一致导致的编译复杂度
#[derive(Clone)]
struct MultiWriter {
    to_stdout: bool,
    file_tx: Option<mpsc::Sender<Cmd>>, // 若需要文件输出则存在
}

struct MultiWriterHandle {
    to_stdout: bool,
    file_tx: Option<mpsc::Sender<Cmd>>,
}

impl Write for MultiWriterHandle {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.to_stdout {
            // 尽量使用标准输出写入；忽略错误避免影响主流程
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

/// 全局日志句柄：支持动态调整级别与优雅关闭
pub struct LoggerHandle {
    _bg: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
    tx: mpsc::Sender<Cmd>,
    // filter_handle: reload::Handle<EnvFilter, Registry>,
}

// impl LoggerHandle {
//     /// 动态调整日志级别（不重建管线）
//     pub fn set_level(&self, level: &str) -> Result<()> {
//         let level = level.to_ascii_lowercase();
//         let spec = match level.as_str() {
//             "error" => "error",
//             "warn" => "warn",
//             "info" => "info",
//             "debug" => "debug",
//             "trace" => "trace",
//             other => other,
//         };
//         let new_filter = EnvFilter::try_new(spec)?;
//         self.filter_handle.reload(new_filter)?;
//         Ok(())
//     }
// }

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

/// 校验 TelemetryConfig 中与日志相关的字段
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

/// 初始化全局日志（在 `main` 最早调用）
/// - JSON/Plain 格式二选一；
/// - 输出支持 stdout/file/both；
/// - 文件输出为异步写入 + 大小轮转；
/// - 自动携带模块、文件、行号与当前 span。
pub fn init_logging(cfg: &TelemetryConfig) -> Result<LoggerHandle> {
    validate_config(cfg)?;
    let default_level = cfg.log_level.to_ascii_lowercase();
    let filter = EnvFilter::try_new(default_level.clone()).unwrap_or_else(|_| EnvFilter::new("info"));
    let (filter_layer, _filter_handle) = reload::Layer::new(filter);

    // 文件轮转线程（仅当需要文件输出）
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

    let multi_writer = MultiWriter { to_stdout: cfg.log_output == "stdout" || cfg.log_output == "both", file_tx: file_tx.clone() };

    // 使用统一构建，保持原有行为不变，仅去重
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

/// 全局初始化版本：保存句柄，确保后台线程存活
static LOGGER_HANDLE: OnceCell<LoggerHandle> = OnceCell::new();

/// 初始化全局日志并保存句柄，多次调用将返回错误
pub fn init_global_logging(cfg: &TelemetryConfig) -> Result<&'static LoggerHandle> {
    let handle = init_logging(cfg)?;
    LOGGER_HANDLE
        .set(handle)
        .map_err(|_| anyhow!("Logger already initialized"))?;
    Ok(LOGGER_HANDLE.get().expect("logger set"))
}
