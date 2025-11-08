mod loader;
pub mod schema;

use anyhow::Result;
use once_cell::sync::OnceCell;
use std::path::Path;
use std::sync::Arc;

static GLOBAL_CONFIG: OnceCell<Arc<schema::Config>> = OnceCell::new();

pub fn init_global_from_file<P: AsRef<Path>>(path: P) -> Result<()> {
    let arc_cfg = loader::load_arc_from_file(path)?;
    GLOBAL_CONFIG
        .set(arc_cfg)
        .map_err(|_| anyhow::anyhow!("Global config already initialized"))?;
    Ok(())
}

pub fn global() -> Arc<schema::Config> {
    GLOBAL_CONFIG
        .get()
        .expect("Global config not initialized")
        .clone()
}
