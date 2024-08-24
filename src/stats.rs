#![deny(unused_imports)]

use std::fs::File;
use std::time::Duration;
use axum::extract::{Query, State};
use crate::{ConfigState};
use crate::common::{ApiError, ApiResponse, ParseError};
use crate::config::{Stats, Tls};
use serde::{Deserialize, Serialize};
use systemstat::{System, Platform, saturating_sub_bytes};
use tklog::{error, warn};
use x509_parser::prelude::*;

#[derive(serde::Deserialize, Clone)]
pub struct StatsParams {
    token: Option<String>,
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[serde_with::skip_serializing_none]
#[derive(Serialize, Deserialize, Debug)]
pub struct Info {
    version: String,
    current_connections: usize,
    rate_limit: usize,
    security_enabled: bool,
    cpu_usage: i32,
    internal_ip: String,
    // compression_enabled: bool,
    memory: u64,
    cert_infos: Option<Vec<CertInfo>>,
}

#[derive(Serialize, Deserialize, Debug)]
struct CertInfo {
    name: String,
    expire_at: String,
}

pub async fn get_count(State(state): State<ConfigState>, Query(params): Query<StatsParams>) -> anyhow::Result<ApiResponse, ApiError> {
    if !check_token(params.token, state.config.stats) {
        return Err(ApiError::Unauthorised)
    }
    Ok(ApiResponse::Count(state.hub.num_client().await.to_string()))
}

pub async fn get_version(State(state): State<ConfigState>, Query(params): Query<StatsParams>) -> anyhow::Result<ApiResponse, ApiError> {
    if !check_token(params.token, state.config.stats) {
        return Err(ApiError::Unauthorised)
    }
    Ok(ApiResponse::Count(VERSION.to_string()))
}

pub async fn get_info(State(state): State<ConfigState>, Query(params): Query<StatsParams>) -> anyhow::Result<ApiResponse, ApiError> {
    if !check_token(params.token, state.config.stats) {
        return Err(ApiError::Unauthorised)
    }
    let sys = System::new();
    let used_memory = match sys.memory() {
        Ok(mem) => saturating_sub_bytes(mem.total, mem.free).as_u64(),
        Err(x) => 0
    };
    let cpu = match sys.cpu_load_aggregate() {
        Ok(cpu)=> {
            tokio::time::sleep(Duration::from_secs(2)).await;
            if let Ok(c) = cpu.done() {
                ((1f32-c.idle) * 100.0) as i32
            } else {
                0
            }
        },
        Err(x) => 0
    };
    let security_enabled = match state.config.security {
        None => false,
        Some(security) => security.enable,
    };
    // let compression_enabled = match state.config.compression {
    //     None => false,
    //     Some(compression) => compression.enable,
    // };
    let mut cert_infos = Vec::new();
    if let Some(tls) = state.config.tls {
        match tls {
            Tls::TlsItem(item) => {
                if let Ok(info) = parse_cert(item.cert.as_str()) {
                    cert_infos.push(info);
                }
            }
            Tls::TlsItems(items) => {
                for item in items {
                    if let Ok(info) = parse_cert(item.cert.as_str()) {
                        cert_infos.push(info);
                    }
                }
            }
        }
    }
    let mut info = Info{
        version: VERSION.to_string(),
        current_connections: state.hub.num_client().await,
        rate_limit: 0,
        security_enabled,
        cpu_usage: cpu,
        internal_ip: state.local_ip,
        // compression_enabled,
        memory: used_memory,
        cert_infos: None,
    };
    if cert_infos.len() > 0 {
        info.cert_infos = Some(cert_infos);
    }
    Ok(ApiResponse::Info(info))
}

pub async fn get_profile(State(state): State<ConfigState>, Query(params): Query<StatsParams>) -> anyhow::Result<ApiResponse, ApiError> {
    if !check_token(params.token, state.config.stats) {
        return Err(ApiError::Unauthorised)
    }
    tokio::task::spawn(async move {
        let mut guard = pprof::ProfilerGuardBuilder::default().frequency(1000)
            .blocklist(&["libc", "libgcc", "pthread", "vdso"])
            .build().unwrap();
        tokio::time::sleep(Duration::from_secs(30)).await;
        if let Ok(report) = guard.report().build() {
            let file = File::create("flamegraph.svg").unwrap();
            report.flamegraph(file).unwrap();
            warn!("profile done")
        };
    });
    Ok(ApiResponse::OK)
}

fn check_token(token: Option<String>, stats: Option<Stats>) -> bool {
    if let Some(stats) = stats {
        if !stats.enable {
            return false
        }
        if stats.token.is_some() {
            return token == stats.token
        }
        return true
    }
    return false
}

fn parse_cert(file_name: &str) -> anyhow::Result<CertInfo> {
    let data = std::fs::read(file_name.clone()).expect("Unable to read file");
    if matches!((data[0], data[1]), (0x30, 0x81..=0x83)) {
        // probably DER
        handle_certificate(&file_name, &data)
    } else {
        // try as PEM
        for (n, pem) in Pem::iter_from_buffer(&data).enumerate() {
            return match pem {
                Ok(pem) => {
                    let data = &pem.contents;
                    handle_certificate(&file_name, data)
                }
                Err(e) => {
                    eprintln!("Error while decoding PEM entry {}: {}", n, e);
                    Err(e.into())
                }
            }
        }
        Err(ParseError::Parse(file_name.to_string(), "not found".to_string()).into())
    }
}

fn handle_certificate(file_name: &str, data: &[u8]) -> anyhow::Result<CertInfo> {
    match parse_x509_certificate(data) {
        Ok((_, x509)) => {
            Ok(CertInfo{
                name: x509.subject().to_string().split('=').next_back().unwrap().to_string(),
                expire_at: x509.validity().not_after.to_datetime().to_string(),
            })
        }
        Err(e) => {
            let s = format!("Error while parsing {}: {}", file_name, e);
            error!(e);
            Err(e.into())
        }
    }
}