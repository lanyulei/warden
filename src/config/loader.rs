use crate::config::schema::Config as AppConfig;
use std::path::Path;
use std::sync::Arc;
use anyhow::{Context, Result};
use config::{Config as RawConfig, Environment, File};

// 从配置文件中获取配置
pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<AppConfig> {
    let default = AppConfig::default();
    let mut builder = RawConfig::builder();

    let default_str = serde_json::to_string(&default)?;
    builder = builder.add_source(File::from_str(&default_str, config::FileFormat::Json));

    let mut env_path = std::env::var("WARDEN_CONFIG_PATH").ok().map(std::path::PathBuf::from);
    if let Some(ref p) = env_path {
        if !p.exists() {
            // 环境变量指定的配置文件不存在，则忽略
            env_path = None;
        }
    }
    let path_ref: &Path = env_path.as_deref().unwrap_or_else(|| path.as_ref());
    if path_ref.exists() {
        builder = builder.add_source(File::from(path_ref));
    }

    // 环境变量覆盖
    builder = builder.add_source(Environment::with_prefix("WARDEN").separator("_"));
    let raw_cfg = builder.build().context("Failed to build config")?;
    let cfg: AppConfig = raw_cfg
        .try_deserialize()
        .context("Failed to deserialize config")?;
    cfg.validate().context("Config validation failed")?;
    Ok(cfg)
}

pub fn load_arc_from_file<P: AsRef<Path>>(path: P) -> Result<Arc<AppConfig>> {
    Ok(Arc::new(load_from_file(path)?))
}