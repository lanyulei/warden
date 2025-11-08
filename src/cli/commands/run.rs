use clap::Parser;

#[derive(Debug, Parser)]
pub struct Run {
    #[arg(
        short,
        long,
        value_name = "FILE",
        default_value = "config.yaml",
        help = "Path to the configuration file"
    )]
    pub config: String,
}

impl Run {
    pub fn execute(&self) {
        // 初始化全局配置
        crate::config::init_global_from_file(self.config.clone()).unwrap();
        // 初始化全局日志（基于配置）
        let cfg = crate::config::global();
        let _ = crate::telemetry::logging::init_global_logging(&cfg.telemetry);
    }
}
