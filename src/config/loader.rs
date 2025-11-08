//! Config loader: load and validate application config from file or environment.

use crate::config::schema::Config as AppConfig;
use anyhow::{Context, Result};
use config::{Config as RawConfig, Environment, File};
use std::path::Path;
use std::sync::Arc;

/// 加载配置文件，支持默认值、环境变量覆盖和校验。
pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<AppConfig> {
    // 默认配置
    let default = AppConfig::default();
    let mut builder = RawConfig::builder();
    let default_str = serde_json::to_string(&default)?;
    builder = builder.add_source(File::from_str(&default_str, config::FileFormat::Json));

    // 优先使用环境变量指定的配置文件
    let mut env_path = std::env::var("WARDEN_CONFIG_PATH")
        .ok()
        .map(std::path::PathBuf::from);
    if let Some(ref p) = env_path {
        if !p.exists() {
            env_path = None; // 不存在则忽略
        }
    }
    let path_ref: &Path = env_path.as_deref().unwrap_or_else(|| path.as_ref());
    if path_ref.exists() {
        builder = builder.add_source(File::from(path_ref));
    }

    // 环境变量覆盖配置项
    builder = builder.add_source(Environment::with_prefix("WARDEN").separator("_"));
    let raw_cfg = builder.build().context("Failed to build config")?;
    let cfg: AppConfig = raw_cfg
        .try_deserialize()
        .context("Failed to deserialize config")?;
    cfg.validate().context("Config validation failed")?;
    Ok(cfg)
}

/// 加载配置并返回 Arc 包装，便于多处共享。
pub fn load_arc_from_file<P: AsRef<Path>>(path: P) -> Result<Arc<AppConfig>> {
    Ok(Arc::new(load_from_file(path)?))
}
