use crate::state::{Profile, Res};
use std::time::Duration;
use ureq::{Agent, Proxy};

const USER_AGENT: &str = "xctcc";
const DEFAULT_INTERVAL_H: u64 = 24;
const MAX_BODY: u64 = 32 * 1024 * 1024;

pub struct Fetched {
    pub body: String,
    pub name: String,
    pub interval_h: u64,
    pub usage: [u64; 4],
}

/// Fixes `https://host/path&token=x`, where the query was glued onto the path.
fn fix_dirty_url(url: &str) -> String {
    let host_end = url.find("://").map_or(0, |i| i + 3);
    let has_path_amp = !url.contains('?') && url[host_end..].contains('&');
    if has_path_amp {
        url.replacen('&', "?", 1)
    } else {
        url.to_string()
    }
}

fn agent(proxy_port: Option<u16>) -> Res<Agent> {
    let proxy = match proxy_port {
        Some(port) => Some(Proxy::new(&format!("http://127.0.0.1:{port}"))?),
        None => None,
    };
    let config = Agent::config_builder()
        .user_agent(USER_AGENT)
        .proxy(proxy)
        .timeout_global(Some(Duration::from_secs(20)))
        .build();
    Ok(config.into())
}

/// Parses `upload=1; download=2; total=3; expire=4`.
fn parse_usage(info: &str) -> [u64; 4] {
    let get = |key: &str| {
        info.split(';')
            .filter_map(|kv| kv.trim().split_once('='))
            .find(|(k, _)| *k == key)
            .and_then(|(_, v)| v.trim().parse().ok())
            .unwrap_or(0)
    };
    [get("upload"), get("download"), get("total"), get("expire")]
}

pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok());
        match (bytes[i], hex.and_then(|h| u8::from_str_radix(h, 16).ok())) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn filename(disposition: &str) -> Option<String> {
    let find = |key: &str| {
        disposition
            .split(';')
            .map(str::trim)
            .find_map(|part| part.strip_prefix(key))
    };
    if let Some(v) = find("filename*=") {
        let encoded = v.rsplit("''").next().unwrap_or(v);
        return Some(percent_decode(encoded.trim_matches('"')));
    }
    find("filename=").map(|v| v.trim_matches('"').to_string())
}

fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split(['/', '?']).next().unwrap_or("订阅").to_string()
}

pub fn validate(body: &str) -> Res<()> {
    let ok = body
        .lines()
        .any(|l| l.starts_with("proxies:") || l.starts_with("proxy-providers:"));
    if ok {
        Ok(())
    } else {
        Err("订阅内容不是 Clash YAML（缺少 proxies / proxy-providers）".into())
    }
}

pub fn fetch(url: &str, proxy_port: Option<u16>) -> Res<Fetched> {
    let url = fix_dirty_url(url.trim());
    let mut resp = agent(proxy_port)?.get(&url).call()?;
    let header = |name: &str| {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(String::from)
    };
    let usage_header = resp
        .headers()
        .iter()
        .find(|(k, _)| k.as_str().ends_with("subscription-userinfo"))
        .and_then(|(_, v)| v.to_str().ok().map(String::from));
    let name = header("content-disposition")
        .as_deref()
        .and_then(filename)
        .unwrap_or_else(|| host_of(&url));
    let interval_h = header("profile-update-interval")
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(DEFAULT_INTERVAL_H);
    let text = resp
        .body_mut()
        .with_config()
        .limit(MAX_BODY)
        .read_to_string()?;
    let body = text.trim_start_matches('\u{feff}').to_string();
    validate(&body)?;
    let usage = usage_header.map_or([0; 4], |h| parse_usage(&h));
    Ok(Fetched {
        body,
        name,
        interval_h,
        usage,
    })
}

impl Profile {
    pub fn apply(&mut self, f: &Fetched, now: u64) {
        self.updated = now;
        self.interval_h = f.interval_h;
        [self.upload, self.download, self.total, self.expire] = f.usage;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixes_query_glued_to_path() {
        assert_eq!(
            fix_dirty_url("https://a.com/sub&token=1&flag=clash"),
            "https://a.com/sub?token=1&flag=clash"
        );
        assert_eq!(
            fix_dirty_url("https://a.com/sub?token=1&x=2"),
            "https://a.com/sub?token=1&x=2"
        );
        assert_eq!(fix_dirty_url("https://a.com/sub"), "https://a.com/sub");
    }

    #[test]
    fn parses_userinfo_header() {
        let usage = parse_usage("upload=1; download=2; total=300; expire=1893456000");
        assert_eq!(usage, [1, 2, 300, 1_893_456_000]);
        assert_eq!(parse_usage("total=5"), [0, 0, 5, 0]);
    }

    #[test]
    fn extracts_filename_from_disposition() {
        assert_eq!(
            filename("attachment; filename=\"plain.yaml\"").as_deref(),
            Some("plain.yaml")
        );
        let rfc5987 = "attachment; filename*=UTF-8''%E6%9C%BA%E5%9C%BA%20A.yaml";
        assert_eq!(filename(rfc5987).as_deref(), Some("机场 A.yaml"));
        assert_eq!(filename("inline"), None);
    }

    #[test]
    fn host_falls_back_from_url() {
        assert_eq!(
            host_of("https://sub.example.com/api/v1?token=x"),
            "sub.example.com"
        );
    }

    #[test]
    fn validates_clash_yaml() {
        assert!(validate("port: 1\nproxies:\n  - name: a\n").is_ok());
        assert!(validate("proxy-providers:\n  p: {}\n").is_ok());
        assert!(validate("c3M6Ly9hYmM=").is_err());
        assert!(validate("  proxies:\n").is_err());
    }
}
