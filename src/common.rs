#![deny(unused_imports)]
use std::fmt::{Debug, Formatter};
use async_trait::async_trait;
use axum::body::Bytes;
use axum::extract::{FromRequest, Request};
use axum::Json;
use axum::response::{IntoResponse, Response};
use http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::stats::Info;

#[serde_with::skip_serializing_none]
#[derive(Serialize, Deserialize, Clone)]
pub struct SignalMsg {
    pub action: Option<String>,
    pub to_peer_id: Option<String>,
    pub to: Option<String>,
    pub from_peer_id: Option<String>,
    pub from: Option<String>,
    pub reason: Option<String>,
    pub data: Option<Value>,
    pub ver: Option<i32>,
}

impl Debug for SignalMsg {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if self.ver.is_some() {
            return write!(f, "<action: {:?}, ver: {:?}>", self.action, self.ver)
        }
        if self.action == Some("reject".to_string()) {
            return write!(f, "<action: {:?}, reason: {:?} from: {:?} to: {:?}>", self.action, self.reason, self.to_peer_id, self.from_peer_id)
        }
        if self.action == Some("ping".to_string()) || self.action == Some("pong".to_string()) {
            return write!(f, "<action: {:?}>", self.action)
        }
        write!(f, "<action: {:?}, to: {:?} from: {:?} data: {:?}>",
               self.action, self.to_peer_id.to_owned().unwrap_or(self.to.to_owned().unwrap_or_default()), self.from_peer_id.to_owned().unwrap_or(self.from.to_owned().unwrap_or_default()), self.data)
    }
}

impl Default for SignalMsg {
    fn default() -> Self {
        Self {
            action: None,
            to_peer_id: None,
            to: None,
            from_peer_id: None,
            from: None,
            reason: None,
            data: None,
            ver: None,
        }
    }
}

impl TryFrom<&str> for SignalMsg {
    type Error = serde_json::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        serde_json::from_str(value)
    }
}

impl TryFrom<&SignalMsg> for String {
    type Error = serde_json::Error;

    fn try_from(value: &SignalMsg) -> Result<Self, Self::Error> {
        serde_json::to_string(value)
    }
}

// impl SignalMsg {
//     pub fn to_string(&self) -> String {
//         serde_json::to_string(self).unwrap()
//     }
// }

impl SignalMsg {
    pub fn to_peer_id(&self) -> Option<&str> {
        if let Some(to) = self.to_peer_id.as_ref() {
            return Some(to)
        }
        if let Some(to) = self.to.as_ref() {
            return Some(to)
        }
        None
    }
}

pub struct ValidatedBody(pub Bytes);

#[async_trait]
impl<S> FromRequest<S> for ValidatedBody
    where
        Bytes: FromRequest<S>,
        S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let body = Bytes::from_request(req, state)
            .await
            .map_err(IntoResponse::into_response)?;

        // do validation...

        Ok(Self(body))
    }
}

pub enum ApiResponse {
    OK,
    Signals(Vec<SignalMsg>),
    SignalVersion(i32),
    Count(String),
    Version(String),
    Info(Info),
}

// 这让 `ApiResponse` 可以被自动转换成一个 `axum Response`。
impl IntoResponse for ApiResponse {
    fn into_response(self) -> Response {
        // 检查枚举变量,返回相应的 HTTP 状态码和数据。
        match self {
            Self::OK => (StatusCode::OK).into_response(),
            Self::Signals(data) => (StatusCode::OK, Json(data)).into_response(),
            Self::SignalVersion(ver) => (StatusCode::OK, Json(SignalMsg{
                action: Some("ver".to_string()),
                ver: Some(ver),
                ..SignalMsg::default()

            })).into_response(),
            Self::Count(count) =>  (StatusCode::OK, count).into_response(),
            Self::Version(ver) =>  (StatusCode::OK, ver).into_response(),
            Self::Info(info) => (StatusCode::OK, Json(info)).into_response(),
        }
    }
}

pub enum ApiError {
    BadRequest,
    Forbidden,
    Unauthorised,
    InternalServerError,
    CONFLICT,
    TokenInvalid
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        // 检查枚举变量,返回相应的 HTTP 状态码和数据。
        match self {
            Self::BadRequest => (StatusCode::BAD_REQUEST).into_response(),
            Self::Forbidden => (StatusCode::FORBIDDEN).into_response(),
            Self::Unauthorised => (StatusCode::UNAUTHORIZED).into_response(),
            Self::CONFLICT => (StatusCode::CONFLICT).into_response(),
            Self::InternalServerError => (StatusCode::INTERNAL_SERVER_ERROR).into_response(),
            Self::TokenInvalid => (StatusCode::UNAUTHORIZED).into_response()
        }
    }
}

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("text is empty")]
    TextEmpty,
    #[error("parse error, msg: `{0}` target: `{1}`")]
    Parse(String, String),
}

pub fn parse_json<S: AsRef<str>>(json_str: S) -> anyhow::Result<SignalMsg> {
    let json_str: &str = json_str.as_ref();
    if json_str.is_empty() {
        return Err(ParseError::TextEmpty.into())
    }
    match serde_json::from_str(json_str) {
        Ok(data) => {
            // 成功反序列化,返回 MyStruct 实例
            Ok(data)
        }
        Err(err) => {
            // 反序列化出错,返回 serde_json::Error
            Err(ParseError::Parse(err.to_string(), json_str.to_string()).into())
        }
    }
}