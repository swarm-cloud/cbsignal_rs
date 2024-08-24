#![deny(unused_imports)]
#![allow(dead_code)]
use std::sync::{Arc, Mutex};
use std::time::{Duration};
use tokio::sync::mpsc::{UnboundedSender};
use tokio::sync::mpsc::Sender;
use tokio::time::Instant;
use crate::common::SignalMsg;

const POLLING_QUEUE_SIZE: usize   = 30;
const POLLING_EXPIRE_LIMIT: u64 = 3 * 60 * 1000;
const WS_EXPIRE_LIMIT: u64 = 11 * 60 * 1000;

fn now() -> Instant {
    // SystemTime::now()
    //     .duration_since(SystemTime::UNIX_EPOCH)
    //     .expect("SystemTime before UNIX EPOCH!")
    //     .as_millis()
    Instant::now()
}

type Queue = Arc<Mutex<Vec<Arc<SignalMsg>>>>;

#[derive(Clone)]
pub struct Client {
    pub peer_id: String,
    pub is_polling: bool,
    pub timestamp: Instant,
    pub msg_queue: Queue,
    pub(crate) ws: Option<UnboundedSender<String>>,
    pub http: Option<Sender<()>>

}

impl Client {

    pub fn new(peer_id: &str, sender: UnboundedSender<String>) -> Self {
        Self {
            peer_id: peer_id.to_string(),
            is_polling: false,
            timestamp: now(),
            msg_queue: Arc::new(Mutex::new(vec![])),
            ws: Some(sender),
            http: None,
        }
    }

    pub fn new_poll(peer_id: &str, sender: Sender<()>) -> Self {
        // let queue = Vec::with_capacity(20);
        // println!("Client::new_poll {} queue {:p}", peer_id, &queue);
        Self {
            peer_id: peer_id.to_string(),
            is_polling: true,
            timestamp: now(),
            msg_queue: Arc::new(Mutex::new(vec![])),
            ws: None,
            http: Some(sender),
        }
    }

    pub fn clear_queue(&mut self) {
        self.msg_queue.lock().unwrap().clear()
    }

    pub fn switch_to_ws(&mut self, sender: UnboundedSender<String>) {
        self.ws = Some(sender);
        self.http = None;
    }

    pub fn switch_to_http(&mut self, sender: Sender<()>) {
        // println!("switch_to_http");
        self.http = Some(sender);
        self.ws = None;
    }

    pub async fn send_version(&mut self, ver: i32) -> bool {
        let msg = SignalMsg {
            action: Some("ver".to_string()),
            ver: Some(ver),
            ..SignalMsg::default()
        };
        self.send_message(Arc::new(msg)).await
    }

    pub async fn send_message(&mut self, msg: Arc<SignalMsg>) -> bool {
        if self.is_polling {
            return self.send_data_polling(msg).await;
        }
        self.send_msg_to_ws(msg).await

        // Err(anyhow!("ws is null"))
    }

    pub fn get_queued_msgs(&mut self) -> Vec<Arc<SignalMsg>> {
        // println!("{} self.msg_queue len {} {:p}", self.peer_id, self.msg_queue.len(), &self.msg_queue);
        // serde_json::to_string(&self.msg_queue).unwrap()
        // "".to_string()
        let msgs = self.msg_queue.lock().unwrap().to_vec();
        self.clear_queue();
        msgs
    }

    pub fn update_ts(&mut self) {
        self.timestamp = now();
    }

    pub fn is_expired(& self, now: Instant) -> bool {
        if self.is_polling {
            return now.duration_since(self.timestamp) > Duration::from_millis(POLLING_EXPIRE_LIMIT)
        }
        return now.duration_since(self.timestamp) > Duration::from_millis(WS_EXPIRE_LIMIT)
    }

    async fn send_data_polling(&mut self, msg: Arc<SignalMsg>) -> bool {
        if self.msg_queue.lock().unwrap().len() >= POLLING_QUEUE_SIZE {
            return true
        }
        self.msg_queue.lock().unwrap().push(msg);
        if let Some(http) = self.http.as_ref() {
            if http.capacity() > 0 {
                return http.send(()).await.is_ok();
            }
        }
        return true
    }

    async  fn send_msg_to_ws(&self, msg: Arc<SignalMsg>) -> bool {
        if let Some(ws) = self.ws.clone() {
            return ws.send(msg.as_ref().try_into().unwrap()).is_ok()
        }
        false
    }

    pub async fn close(&mut self) {
        if self.is_polling {
            if let Some(http) = self.http.clone() {
                http.closed().await
            }
        } else {
            if let Some(ws) = self.ws.clone() {
                ws.closed().await
            }
        }
    }
}

