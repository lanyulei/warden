use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub basic: BasicConfig,
    pub grpc: GrpcConfig,
    pub tls: TlsConfig,
    pub telemetry: TelemetryConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            basic: BasicConfig::default(),
            grpc: GrpcConfig::default(),
            tls: TlsConfig::default(),
            telemetry: TelemetryConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicConfig {
    pub plugin_dir: String, // 插件目录
    pub sqlite_path: String, // SQLite数据库文件路径
    pub max_memory_mb: u32, // 最大内存，单位 mb
    pub max_cpu_percent: u32, // 最大CPU使用百分比
    pub max_file_handles: u32, // 最大文件句柄数
}

impl Default for BasicConfig {
    fn default() -> Self {
        Self {
            plugin_dir: "./plugins".to_string(),
            sqlite_path: "./data/db.sqlite".to_string(),
            max_memory_mb: 32,
            max_cpu_percent: 3,
            max_file_handles: 32,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrpcConfig {
    pub masters: Vec<String>, // master地址列表
    pub connect_timeout_secs: u64, // 连接超时时间，单位 秒
    pub max_receive_message_mb: u32, // 最大接收消息大小，单位 mb
    pub max_send_message_mb: u32, // 最大发送消息大小，单位 mb
    pub keepalive: KeepaliveConfig, // 保持连接的配置
    pub reconnect: ReconnectConfig, // 重连的配置
}

impl Default for GrpcConfig {
    fn default() -> Self {
        Self {
            masters: vec!["127.0.0.1:50051".to_string()],
            connect_timeout_secs: 5,
            max_receive_message_mb: 16,
            max_send_message_mb: 16,
            keepalive: KeepaliveConfig::default(),
            reconnect: ReconnectConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeepaliveConfig {
    pub time_secs: u64,  // 发送keepalive的时间间隔，单位 秒
    pub timeout_secs: u64, // 等待keepalive响应的时间，单位 秒
    pub permit_without_calls: bool, // 是否允许在没有活动调用时发送keepalive
}

impl Default for KeepaliveConfig {
    fn default() -> Self {
        Self {
            time_secs: 30,
            timeout_secs: 5,
            permit_without_calls: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconnectConfig {
    pub max_attempts: u32, // 最大重试次数
    pub initial_backoff_secs: u64, // 初始重试间隔，单位 秒
    pub max_backoff_secs: u64, // 最大重试间隔，单位 秒
    pub backoff_multiplier: f64, // 重试间隔乘数
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_backoff_secs: 1,
            max_backoff_secs: 20,
            backoff_multiplier: 2.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    pub enable: bool,  // 是否启用tls
    pub ca_file: String, // CA证书文件路径
    pub cert_file: String, // 客户端证书文件路径
    pub key_file: String, // 客户端私钥文件路径
    pub server_name_override: String, // 服务器名称覆盖
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            enable: false,            // 默认不启用TLS
            ca_file: "".to_string(),
            cert_file: "".to_string(),
            key_file: "".to_string(),
            server_name_override: "".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    pub log_level: String, // 日志级别
    pub log_format: String, // 日志格式 (如 json, plain)
    pub log_output: String, // 日志输出位置，如 stdout, file
    pub log_file: String, // 日志文件路径，当 log_output 为 file 时生效
    pub log_rotation: LogRotationConfig, // 日志轮转配置

    pub metrics_port: u16, // 指标端口
    pub metrics_path: String, // 指标路径
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            log_level: "info".to_string(),
            log_format: "json".to_string(),
            log_output: "stdout".to_string(),
            log_file: "./log/agent.log".to_string(),
            log_rotation: LogRotationConfig::default(),
            metrics_port: 9090,
            metrics_path: "/metrics".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRotationConfig {
    pub max_size_mb: u32, // 最大日志文件大小，单位 mb
    pub max_files: u32, // 最大日志文件数量
    pub compress: bool, // 是否压缩旧日志文件
}

impl Default for LogRotationConfig {
    fn default() -> Self {
        Self {
            max_size_mb: 100,
            max_files: 7,
            compress: true,
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.grpc.masters.len() == 0 {
            return Err(anyhow!("masters is empty"));
        }
        if self.basic.sqlite_path.is_empty() {
            return Err(anyhow!("sqlite_path is empty"));
        }
        match self.telemetry.log_level.to_ascii_lowercase().as_str() {
            "error" | "warn" | "info" | "debug" | "trace" => {}
            other => return Err(anyhow!("invalid log_level: {}", other)),
        }
        match self.telemetry.log_format.to_ascii_lowercase().as_str() {
            "json" | "plain" => {}
            other => return Err(anyhow!("invalid log_format: {}", other)),
        }
        match self.telemetry.log_output.to_ascii_lowercase().as_str() {
            "stdout" | "file" | "both" => {}
            other => return Err(anyhow!("invalid log_output: {}", other)),
        }
        if (self.telemetry.log_output == "file" || self.telemetry.log_output == "both") && self.telemetry.log_file.trim().is_empty() {
            return Err(anyhow!("log_file required when output=file/both"));
        }
        Ok(())
    }
}
