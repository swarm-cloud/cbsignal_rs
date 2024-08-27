
mod config;
mod logger;
mod client;
mod hub;
mod utils;
mod common;
mod stats;
mod handler;

use std::fmt::{Debug};
use std::str;
use std::string::String;
use std::net::{Ipv6Addr, SocketAddr};
use std::sync::Arc;
use bpaf::Bpaf;
use tklog::{warn};
use tokio::task;
use crate::hub::Hub;
use http::Method;
use crate::utils::{get_version_num};
use tower_http::cors::{Any, CorsLayer};
use axum::{Router};
use axum::routing::get;
use crate::config::{Config, Port, Security, Tls, TlsItem};
use futures::future;
use axum_server::tls_rustls::RustlsConfig;
use crate::stats::{get_count, get_info, get_profile, get_version};
use local_ip_address::local_ip;
use crate::handler::{handle_http_or_websocket, handle_post};
use tower_http::{services::{ServeFile}};
use ratelimit::Ratelimiter;
use std::time::Duration;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Bpaf)]
#[bpaf(options)]
pub struct Arguments {
    #[bpaf(
        long("config"),
        short('c'),
        fallback("config.yaml".to_string())
    )]
    pub path: String,
}

#[derive(Clone)]
pub struct AppState {
    pub hub: Hub,
    pub version_number: i32,
    pub security: Option<Security>,
    pub ratelimit: Option<Arc<Ratelimiter>>,
}

#[derive(Clone)]
pub struct ConfigState {
    pub hub: Hub,
    pub config: Config,
    pub local_ip: String
}

#[tokio::main]
async fn main() {
    console_subscriber::init();
    let opts = arguments().run();
    // println!("path {:}", opts.path);
    warn!("version", VERSION);
    let config = config::parse(opts.path).expect("parse config failed");
    let ratelimiter: Option<Arc<Ratelimiter>>  = match config.ratelimit {
        None => { None }
        Some(ref r) => {
            match r.enable {
                true => {Some(Arc::new(Ratelimiter::builder(r.max_rate, Duration::from_secs(1))
                    .max_tokens(r.max_rate)
                    .initial_available(r.max_rate)
                    .build()
                    .unwrap()))
                }
                false => {None}
            }
        }
    };
    let app_state = AppState {
        hub: Hub::new(),
        version_number: get_version_num(VERSION),
        security: config.security.clone(),
        ratelimit: ratelimiter,
    };
    let config_state = ConfigState {
        hub: app_state.hub.clone(),
        config: config.clone(),
        local_ip: local_ip().unwrap().to_string(),
    };
    logger::init(config.log);
    let app = Router::new()
        // .layer(cors_layer)
        .route("/", get(handle_http_or_websocket).post(handle_post).with_state(app_state.clone()))
        .route("/count", get(get_count).with_state(config_state.clone()))
        .route("/version", get(get_version).with_state(config_state.clone()))
        .route("/info", get(get_info).with_state(config_state.clone()))
        .route("/profile", get(get_profile).with_state(config_state.clone()))
        .route_service("/flame.svg", ServeFile::new("flamegraph.svg"))
        .layer(CorsLayer::new().allow_origin(Any).allow_methods([Method::GET, Method::POST]));

    let mut server_task = tokio::spawn(async  move {
        // run it
        let mut tasks = Vec::with_capacity(2);
        if let Some(port) = config.port {
            match port {
                Port::Port(port) => {
                    tasks.push(task::spawn(listen_to_http(port, app.clone())));
                }
                Port::Ports(ports) => {
                    for port in ports {
                        tasks.push(task::spawn(listen_to_http(port, app.clone())));
                    }
                }
            }
        }
        if let Some(tls) = config.tls {
            match tls {
                Tls::TlsItem(item) => {
                    tasks.push(task::spawn(listen_to_https(item, app.clone())));
                }
                Tls::TlsItems(items) => {
                    for item in items {
                        tasks.push(task::spawn(listen_to_https(item, app.clone())));
                    }
                }
            }
        }

        future::join_all(tasks).await;
    });

    tokio::select! {
        _ = &mut server_task => {
            warn!("server exited, stopping");
            // sender_task.abort();
        }
        _ = tokio::signal::ctrl_c() => {
            // sender_task.abort();
            server_task.abort();
        }
    }

}

async fn listen_to_http(port: u16, app: Router) -> std::io::Result<()> {
    let addr = SocketAddr::from((Ipv6Addr::UNSPECIFIED, port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap();
    warn!("http listening on", listener.local_addr().unwrap());
    axum::serve(listener, app).await
}

async fn listen_to_https(tls: TlsItem, app: Router) -> std::io::Result<()> {
    let config = RustlsConfig::from_pem_file(tls.cert, tls.key)
        .await
        .unwrap();
    // run https server
    // let addr = SocketAddr::from(([127, 0, 0, 1], tls.port));
    let addr = SocketAddr::from((Ipv6Addr::UNSPECIFIED, tls.port));
    warn!("https listening on", addr);
    axum_server::bind_rustls(addr, config)
        .serve(app.into_make_service())
        .await
}

#[cfg(test)]            // 这里配置测试模块
mod tests {

    #[test]             // 具体的单元测试用例
    fn it_works() {

    }
}



