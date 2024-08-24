
use anyhow::Result;
use serde::Deserialize;
use std::fs::File;
use std::io::Read;
use std::path::Path;

#[derive(Deserialize, Debug, Clone)]
pub enum LogLevel {
    DEBUG,
    INFO,
    WARN,
    ERROR,
    FATAL,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Log {
    pub writers: String,
    pub logger_level: LogLevel,
    pub logger_dir: String,
    pub log_rotate_date: u32,
    pub log_rotate_size: u64,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum Tls {
    TlsItem(TlsItem),
    TlsItems(Vec<TlsItem>),
}

#[derive(Deserialize, Debug, Clone)]
pub struct TlsItem {
    pub port: u16,
    pub cert: String,
    pub key: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Ratelimit {
    pub enable: bool,
    pub max_rate: u64,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Stats {
    pub enable: bool,
    pub token: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Compression {
    pub enable: bool,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum Port {
    Port(u16),
    Ports(Vec<u16>),
}

#[derive(Deserialize, Debug, Clone)]
pub struct Security {
    pub enable: bool,
    pub maxTimeStampAge: u64,
    pub token: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    pub log: Log,
    pub port: Option<Port>,
    pub tls: Option<Tls>,
    pub ratelimit: Option<Ratelimit>,
    pub stats: Option<Stats>,
    pub compression: Option<Compression>,
    pub security: Option<Security>,
}

pub fn parse<P: AsRef<Path>>(path: P) -> Result<Config> {
    // 打开YAML文件
    let mut file = File::open(path)?;

    // 读取文件内容
    let mut content = String::new();
    file.read_to_string(&mut content)?;

    // 将字符串解析为Rust结构体
    let config: Config = serde_yaml::from_str(&content)?;

    // println!("config: {:?}", config);

    Ok(config)
}
