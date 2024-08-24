#![deny(unused_imports)]
use std::sync::{Arc, Mutex};
// use dashmap::DashMap;
use tklog::{ warn};
use tokio::time::{interval_at, Duration, Instant};
use crate::client::Client;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::collections::HashMap;
use serde_json::Value;
use crate::common::SignalMsg;

#[derive(Clone)]
pub struct Hub {
    // map: Arc<DashMap<String, Client>>,
    map: Arc<Mutex<HashMap<String, Client>>>,
    filter: LruCache<String, ()>,
}

impl Hub {

    pub fn new() -> Self {
        let s = Self {
            // map: Arc::new(DashMap::new()),
            map: Arc::new(Mutex::new(HashMap::new())),
            filter: LruCache::new(NonZeroUsize::new(6000).unwrap()),
        };
        let cloned = s.clone();
        tokio::spawn(async move {
            cloned.check_conns().await;
        });
        s
    }

    async fn check_conns(&self) {
        let start = Instant::now() + Duration::from_secs(6*60);
        let mut timer = interval_at(start, Duration::from_secs(6*60));
        loop {
            timer.tick().await;
            warn!("定时任务执行时间:", format!("{:?}", Instant::now()));
            let now = Instant::now();
            let mut ws_count_removed = 0;
            let mut http_count_removed = 0;
            let mut ws_count = 0;
            let mut http_count = 0;
            let mut clients_to_remove = Vec::new();
            for (_, client) in self.map.lock().unwrap().iter() {
                if client.is_expired(now) {
                    clients_to_remove.push(client.clone());
                    if client.is_polling {
                        http_count_removed += 1;
                    } else {
                        ws_count_removed += 1;
                    }
                } else {
                    if client.is_polling {
                        http_count += 1;
                    } else {
                        ws_count += 1;
                    }
                }
            }
            for cli in &clients_to_remove {
                cli.clone().close().await;
                self.map.lock().unwrap().remove(&cli.peer_id);
            }
            if ws_count_removed > 0 || http_count_removed > 0 {
                warn!("check cmap finished, closed clients: ws", ws_count_removed, "polling", http_count_removed)
            }
            warn!("current clients ws", ws_count, "polling", http_count);
        }
    }

    pub async fn do_register(&self, client: Client) {
        // println!("{} do_register", client.peer_id);
        // self.map.lock().insert(client.peer_id.clone(), client);
        self.map.lock().unwrap().insert(client.peer_id.clone(), client);
    }

    pub async fn do_unregister(&self, peer_id: &str) -> bool {
        self.map.lock().unwrap().remove(peer_id).is_some()
    }

    pub async fn num_client(&self) -> usize {
        self.map.lock().unwrap().len()
    }

    pub async fn has_client(&self, peer_id: &str) -> bool {
        self.map.lock().unwrap().contains_key(peer_id)
    }

    pub async fn process_message(&mut self, mut msg: Arc<SignalMsg>, peer_id: &str) {
        if let Some(action) = &msg.action {
            match action.as_ref() {
                "ping" | "heartbeat" => {
                    self.process_ping(peer_id).await;
                }
                action => {
                    let to_peer_id = match msg.to_peer_id() {
                        None => { return; }
                        Some(to) => to
                    };
                    let msg = Arc::new(SignalMsg{
                        action: msg.action.clone(),
                        from_peer_id: Some(peer_id.to_string()),
                        data: msg.data.clone(),
                        ..SignalMsg::default()
                    });
                    let key = key_for_filter(peer_id, to_peer_id);
                    if self.filter.contains(&key) {
                        return;
                    }
                    // println!("from {} to {}", peer_id, to_peer_id.as_str());
                    let target = self.get_client(to_peer_id).await;
                    // if target.is_none() {
                    //     println!("target.is_none");
                    // }
                    match action {
                        "signal" => {
                            self.process_signal(target, msg, to_peer_id, peer_id, key.as_str()).await;
                        }
                        "signals" => {
                            self.process_signals(target, msg, to_peer_id, peer_id, key.as_str()).await;
                        }
                        "reject" => {
                            self.process_reject(target, msg, key.as_str()).await;
                        }
                        _ => {
                            warn!("unknown action {:}", action);
                        }
                    }
                }
            }
        } else {
            if let Some(mut peer) = self.get_client(peer_id).await {
                peer.update_ts();
            }
        }

    }

    async fn process_signals(&mut self, target: Option<Client>, msg: Arc<SignalMsg>, to_peer_id: &str, peer_id: &str, key: &str) {
        if let Some(data) = msg.data.as_ref() {
            match data {
                Value::Array(array) => {
                    for item in array {
                        let json = SignalMsg {
                            action: Some("signal".to_string()),
                            from_peer_id: msg.from_peer_id.to_owned(),
                            data: Some(item.to_owned()),
                            ..SignalMsg::default()
                        };
                        if !self.process_signal(target.clone(), Arc::new(json), to_peer_id, peer_id, key).await {
                            return;
                        }
                    }
                }
                _ => {
                    warn!("process_signals data type wrong");
                }
            }
        }

    }

    async fn process_signal(&mut self, target: Option<Client>, msg: Arc<SignalMsg>, to_peer_id: &str, peer_id: &str, key: &str) -> bool {
        if let Some(target) = target {
            let success = self.send_json_to_client(target, msg).await;
            if !success {
                // println!("send_json_to_client not success");
                let peer = self.get_client(peer_id).await;
                self.handle_peer_not_found(peer, to_peer_id, key).await;
            }
            return success;
        }
        let peer = self.get_client(peer_id).await;
        // println!("handle_peer_not_found {}", peer.clone().unwrap().peer_id);
        self.handle_peer_not_found(peer, to_peer_id, key).await;
        return false
    }

    async fn process_reject(&mut self, target: Option<Client>, msg: Arc<SignalMsg>, key: &str) {
        if let Some(target) = target {
            self.filter.put(key.to_string(), ());
            self.send_json_to_client(target, msg).await;
        }
    }

    async fn handle_peer_not_found(&mut self, client: Option<Client>, to_peer_id: &str, key: &str) {
        self.filter.put(key.to_string(), ());
        let msg = SignalMsg {
            action: Some("signal".to_string()),
            from_peer_id: Some(to_peer_id.to_string()),
            ..SignalMsg::default()
        };
        if let Some(client) = client {
            // println!("{} handle_peer_not_found", client.peer_id);
            self.send_json_to_client(client, Arc::new(msg)).await;
        }

    }

    async fn process_ping(&mut self, peer_id: &str) {
        if let Some(mut peer) = self.get_client(peer_id).await {
            peer.update_ts();
            let msg = SignalMsg {
                action: Some("pong".to_string()),
                ..SignalMsg::default()
            };
            if !peer.send_message(Arc::new(msg)).await {
                self.do_unregister(peer_id).await;
            }
        }
    }

    pub async fn get_client(&mut self, peer_id: &str) -> Option<Client> {
        // match self.map.get_mut(peer_id) {
        match self.map.lock().unwrap().get(peer_id) {
            None => {
                None
            }
            Some(value) => {
                Some(value.clone())
            }
        }
    }

    async fn send_json_to_client(&self, mut target: Client, msg: Arc<SignalMsg>) -> bool {
        if !target.send_message(msg).await {
            // warn!("send msg to", target.peer_id, "error, polling", target.is_polling);
            self.do_unregister(target.peer_id.as_str()).await;
            return false;
        }
        true
    }

    pub async fn remove_polling(&self, mut target: Client) {
        target.http = None;
        self.map.lock().unwrap().insert(target.peer_id.clone(), target);
    }
}

fn key_for_filter(from: &str, to: &str) -> String {
    format!("{:}{:}", from, to)
    // from.to_owned() +to
}