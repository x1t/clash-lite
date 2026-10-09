use crate::{
    core::{CTL_HOST, CTL_PORT},
    state::Res,
};
use serde_json::{Value, json};
use std::time::Duration;
use ureq::Agent;

const DELAY_URL: &str = "https://www.gstatic.com/generate_204";

pub struct Api {
    agent: Agent,
    auth: String,
}

pub fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

impl Api {
    pub fn new(secret: &str) -> Self {
        let agent: Agent = Agent::config_builder()
            .proxy(None)
            .timeout_global(Some(Duration::from_secs(15)))
            .build()
            .into();
        Self {
            agent,
            auth: format!("Bearer {secret}"),
        }
    }

    fn url(path: &str) -> String {
        format!("http://{CTL_HOST}:{CTL_PORT}{path}")
    }

    fn get(&self, path: &str) -> Res<Value> {
        let mut resp = self
            .agent
            .get(Self::url(path))
            .header("Authorization", &self.auth)
            .call()?;
        Ok(resp.body_mut().read_json()?)
    }

    pub fn version(&self) -> Res<Value> {
        self.get("/version")
    }

    pub fn configs(&self) -> Res<Value> {
        self.get("/configs")
    }

    pub fn proxies(&self) -> Res<Value> {
        self.get("/proxies")
    }

    pub fn patch_configs(&self, body: &Value) -> Res<()> {
        let req = self
            .agent
            .patch(Self::url("/configs"))
            .header("Authorization", &self.auth);
        req.send_json(body)?;
        Ok(())
    }

    pub fn set_mode(&self, mode: &str) -> Res<()> {
        self.patch_configs(&json!({ "mode": mode }))
    }

    pub fn select(&self, group: &str, node: &str) -> Res<()> {
        let url = Self::url(&format!("/proxies/{}", encode(group)));
        let req = self.agent.put(url).header("Authorization", &self.auth);
        req.send_json(json!({ "name": node }))?;
        Ok(())
    }

    pub fn group_delay(&self, group: &str) -> Res<()> {
        let path = format!(
            "/group/{}/delay?url={}&timeout=5000",
            encode(group),
            encode(DELAY_URL)
        );
        self.get(&path).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::encode;

    #[test]
    fn encodes_group_names() {
        assert_eq!(encode("Proxy-1_a.b~c"), "Proxy-1_a.b~c");
        assert_eq!(
            encode("🚀 节点选择"),
            "%F0%9F%9A%80%20%E8%8A%82%E7%82%B9%E9%80%89%E6%8B%A9"
        );
    }
}
