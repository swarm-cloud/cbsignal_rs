use std::time::{SystemTime, UNIX_EPOCH};
use hmac::{Hmac, Mac};
use md5::{Md5};
use tklog::warn;

type HmacMd5 = Hmac<Md5>;

pub fn get_version_num<S: AsRef<str>>(ver: S) -> i32 {
    let ver: &str = ver.as_ref();
    let mut digs = ver.split('.');
    let major = digs.next().unwrap_or_default().parse::<i32>().unwrap_or(0);
    let minor = digs.next().unwrap_or_default().parse::<i32>().unwrap_or(0);
    major * 10 + minor
}

pub fn check_token(
    id: &str,
    query_token: Option<String>,
    real_token: String,
    max_timestamp_age: u64,
) -> bool {
    let query_token = query_token.unwrap_or("".to_string());
    if query_token.is_empty() || max_timestamp_age == 0 {
        return false;
    }

    let tokens: Vec<&str> = query_token.split('-').collect();
    if tokens.len() < 2 {
        return false;
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let sign = tokens[0];
    let ts_str = tokens[1];
    let ts = ts_str.parse::<u64>().unwrap();

    if ts < now.saturating_sub(max_timestamp_age) || ts > now + max_timestamp_age {
        warn!("ts expired for", now - ts);
        return false;
    }

    let mut hmac = HmacMd5::new_from_slice(real_token.as_bytes()).unwrap();
    hmac.update(ts_str.as_bytes());
    hmac.update(id.as_bytes());
    let real_sign = hex::encode(&hmac.finalize().into_bytes())[..8].to_string();

    if sign != real_sign {
        warn!("token not match, sign", sign, "real_sign", real_sign);
        return false;
    }

    true
}

