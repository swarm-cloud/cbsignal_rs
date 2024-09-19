#![deny(unused_imports)]
use std::str::from_utf8;
use std::sync::Arc;
use std::time::Duration;
use axum::body::Body;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use http::StatusCode;
use tokio::sync::mpsc;
use tokio::time::timeout;
use crate::{AppState};
use crate::client::Client;
use crate::common::{ApiError, ApiResponse, parse_json, SignalMsg, ValidatedBody};
use axum::Json as JSON;
use fastwebsockets::{FragmentCollectorRead, Frame, OpCode, Payload, upgrade, WebSocketError};
use tklog::{error};
use crate::hub::Hub;
use crate::utils::check_token;

#[derive(serde::Deserialize, Clone)]
pub struct SearchParams {
    id: String,
    token: Option<String>,
    hello: Option<String>,
}

#[axum::debug_handler]
pub async fn handle_post(State(mut state): State<AppState>,
                     Query(params): Query<SearchParams>, ValidatedBody(payload): ValidatedBody) -> Result<ApiResponse, ApiError> {
    if params.id.is_empty() || params.id.len() < 6 {
        return Err(ApiError::Unauthorised)
    }
    if !check_sign(&state, &params) {
        return Err(ApiError::TokenInvalid)
    }
    if !check_ratelimit(&state) {
        return Err(ApiError::InternalServerError)
    }
    let is_hello = params.hello.is_some();
    let id = params.id.as_str();
    let client = state.hub.get_client(id).await;
    if client.is_some() && !client.unwrap().is_polling {
        return Err(ApiError::CONFLICT)
    }
    if is_hello {
        return Ok(ApiResponse::SignalVersion(state.version_number))
    }
    // println!("{:?}", payload);
    if let Ok(data) = serde_json::from_slice::<Vec<SignalMsg>>(payload.as_ref()) {
        for msg in data {
            // println!("{:?}", msg);
            state.hub.process_message(Arc::new(msg), id).await;
        }
    }
    Ok(ApiResponse::OK)
}

pub async fn handle_long_polling(mut state: AppState, id: &str) -> Response<Body> {
    let (tx, mut rx) = mpsc::channel::<()>(1);
    let mut client = match state.hub.get_client(id).await  {
        None => {
            let cli = Client::new_poll(id, tx);
            join(cli.clone(), &state.hub).await;
            cli
        }
        Some(mut cli) => {
            if !cli.is_polling {
                return handle_error("", StatusCode::CONFLICT).into_response()
            }
            // println!("cli.msg_queue len {}", cli.msg_queue.lock().unwrap().len());
            if cli.msg_queue.lock().unwrap().len() > 0 {
                let t = cli.get_queued_msgs();
                return JSON(t).into_response()
            }
            cli.switch_to_http(tx);
            join(cli.clone(), &state.hub).await;
            cli
        }
    };

    let result = timeout(Duration::from_secs(60), async {
        // println!(" rx await!!!!");
        rx.recv().await;
        // println!(" rx.recv()");
        let t = client.get_queued_msgs();
        // client.clear_queue();
        // println!("get_queued_msgs {}", t);
        t

        // "Task result"
    }).await;
    state.hub.remove_polling(client).await;
    return match result {
        Ok(result) => {
            // println!("Task completed successfully: {:?}", result);
            // result.into_response()
            // let str = client.get_queued_msgs();
            // client.clear_queue();
            JSON(result).into_response()
        }
        Err(err) => {
            Response::default()
        }
    }
}

async fn handle_socket(mut fut: upgrade::UpgradeFut, mut state: AppState, params: SearchParams) -> Result<(), WebSocketError> {
    let ws = fut.await?;
    let (sender_tx, mut sender_rx) = mpsc::unbounded_channel();
    let version = state.version_number;
    let peer_id = params.id.clone();
    let client = Client::new(&peer_id, sender_tx.clone());
    let mut hub = state.hub.clone();
    join(client.clone(), &state.hub).await;
    let (rx, mut tx) = ws.split(tokio::io::split);
    let mut rx = FragmentCollectorRead::new(rx);
    if !check_sign(&state, &params) {
        return tx.write_frame(Frame::close(4000, "token is not valid".as_bytes())).await
    }
    if !check_ratelimit(&state) {
        return tx.write_frame(Frame::close(5000, "reach ratelimit".as_bytes())).await
    }
    tokio::task::spawn(async move {
        loop {
            // Empty send_fn is fine because the benchmark does not create obligated writes.
            let frame = match rx
                .read_frame::<_, WebSocketError>(&mut move |_| async move {
                    Ok(())
                    // unreachable!();
                }).await {
                Ok(f) => f,
                Err(_) => { break  }
            };
            match frame.opcode {
                OpCode::Close => break,
                OpCode::Ping => {
                    match hub.get_client(params.id.as_str()).await  {
                        None => {}
                        Some(mut cli) => {
                            cli.update_ts();
                        }
                    };
                }
                OpCode::Text => {
                    let byte_slice = frame.payload.as_ref();
                    match from_utf8(byte_slice) {
                        Ok(text) => {
                            match parse_json(text) {
                                Ok(parsed) => {
                                    hub.process_message(Arc::new(parsed), params.id.as_str()).await;
                                }
                                Err(err) => {
                                    // match err.downcast_ref::<ParseJsonError>() {
                                    //     None => {}
                                    //     Some(ParseJsonError::TextEmpty) => {
                                    //         error!("TextEmpty");
                                    //     }
                                    //     Some(ParseJsonError::Parse(_, _)) => {
                                    //         error!(err);
                                    //     }
                                    // }
                                    break;
                                }
                            }
                        }
                        Err(err) => {
                            error!(err);
                        }
                    }

                }
                OpCode::Binary => {
                    error!("ws recv binary data");
                    break
                }
                _ => {}
            }
        }
        let _ = sender_tx.send("".to_string());
    });
    let msg = &SignalMsg {
        action: Some("ver".to_string()),
        ver: Some(version),
        ..SignalMsg::default()
    };
    if tx.write_frame(Frame::text(Payload::Owned(serde_json::to_vec(msg).unwrap()))).await.is_ok() {
        while let Some(msg) = sender_rx.recv().await {
            if msg == "" {
                break
            }
            if tx.write_frame(Frame::text(Payload::Owned(msg.into_bytes()))).await.is_err() {
                break
            }
        }
    }
    leave(peer_id.as_str(), &state.hub).await;
    Ok(())
}

pub async fn handle_http_or_websocket(
    ws: Option<upgrade::IncomingUpgrade>,
    State(state): State<AppState>,
    Query(params): Query<SearchParams>
) -> impl IntoResponse {
    if params.id.is_empty() || params.id.len() < 6 {
        return handle_error("id is not valid", StatusCode::UNAUTHORIZED).into_response()
    }
    if let Some(ws) = ws {
        let (response, fut) = ws.upgrade().unwrap();
        tokio::task::spawn(async move {
            if let Err(e) = tokio::task::unconstrained(handle_socket(fut, state, params)).await {
                // match e {
                //     WebSocketError::ConnectionClosed => {}
                //     _ => {error!("Error in websocket connection", e);}
                // }
            }
        });
        return response.into_response()
    }
    handle_long_polling(state, params.id.as_str()).await.into_response()
}

async fn join(cli: Client, hub: &Hub) {
    hub.do_register(cli).await;
}

async fn leave(peer_id: &str, hub: &Hub) {
    hub.do_unregister(peer_id).await;
    // println!("Disconnected {peer_id}");
}

fn handle_error(msg: &str, code: StatusCode) -> impl IntoResponse {
    (code, format!("{:?}", msg)).into_response()
}

fn handle_internal_error() -> impl IntoResponse {
    handle_error("", StatusCode::INTERNAL_SERVER_ERROR)
}

fn check_sign(state: &AppState, params: &SearchParams) -> bool {
    let params = params.clone();
    if let Some(security) = state.security.clone() {
        if security.enable && !check_token(params.id.as_str(), params.token, security.token, security.maxTimeStampAge) {
            return false
        }
    }
    true
}

fn check_ratelimit(state: &AppState) -> bool {
    if let Some(limiter) = state.ratelimit.clone() {
        if limiter.try_wait().is_err() {
            return false
        }
    }
    return true
}