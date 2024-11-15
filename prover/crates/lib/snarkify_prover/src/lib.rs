pub mod types;

// Mirrors the SnarkifyProver from the [scroll-proving-agent](https://github.com/snarkify/scroll-proving-agent/blob/main/src/prover.rs#L27)
// Not importing it because we don't need some of the Scroll-related logic.

use reqwest::{header::CONTENT_TYPE, Url};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub base_url: String,
    pub api_key: String,
    pub retry_count: u32,
    pub retry_wait_time_sec: u64,
    pub connection_timeout_sec: u64,
}

#[derive(Clone, Debug)]
pub struct Prover {
    base_url: String,
    api_key: String,
    send_timeout: Duration,
    client: ClientWithMiddleware,
}

impl Prover {
    pub fn new(cfg: Config) -> Self {
        let retry_wait_duration = Duration::from_secs(cfg.retry_wait_time_sec);
        let retry_policy = ExponentialBackoff::builder()
            .retry_bounds(retry_wait_duration / 2, retry_wait_duration)
            .build_with_max_retries(cfg.retry_count);
        let client = ClientBuilder::new(reqwest::Client::new())
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build();

        Self {
            base_url: cfg.base_url,
            api_key: cfg.api_key,
            send_timeout: Duration::from_secs(cfg.connection_timeout_sec),
            client,
        }
    }

    fn build_url(&self, method: &str) -> anyhow::Result<Url> {
        let full_url = format!("{}{}", self.base_url, method);
        Url::parse(&full_url)
            .map_err(|e| anyhow::anyhow!("Failed to parse URL '{}': {}", full_url, e))
    }

    pub async fn get_with_token<Resp>(&self, method: &str) -> anyhow::Result<Resp>
    where
        Resp: serde::de::DeserializeOwned,
    {
        let url = self.build_url(method)?;
        log::info!("[Snarkify Client], {method}, sent request");
        let response = self
            .client
            .get(url)
            .header(CONTENT_TYPE, "application/json")
            .header("X-Api-Key", &self.api_key)
            .timeout(self.send_timeout)
            .send()
            .await?;

        let status = response.status();
        if !(status >= http::status::StatusCode::OK && status <= http::status::StatusCode::ACCEPTED)
        {
            anyhow::bail!("[Snarkify Client], {method}, status not ok: {}", status)
        }

        let response_body = response.text().await?;

        log::info!("[Snarkify Client], {method}, received response");
        log::debug!("[Snarkify Client], {method}, response: {response_body}");
        serde_json::from_str(&response_body).map_err(|e| anyhow::anyhow!(e))
    }

    pub async fn post_with_token<Req, Resp>(&self, method: &str, req: &Req) -> anyhow::Result<Resp>
    where
        Req: ?Sized + Serialize,
        Resp: serde::de::DeserializeOwned,
    {
        let url = self.build_url(method)?;
        let request_body = serde_json::to_string(req)?;
        log::info!("[Snarkify Client], {method}, sent request");
        log::debug!("[Snarkify Client], {method}, request: {request_body}");
        let response = self
            .client
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .header("X-Api-Key", &self.api_key)
            .body(request_body)
            .timeout(self.send_timeout)
            .send()
            .await?;

        let status = response.status();
        if !(status >= http::status::StatusCode::OK && status <= http::status::StatusCode::ACCEPTED)
        {
            anyhow::bail!("[Snarkify Client], {method}, status not ok: {}", status)
        }

        let response_body = response.text().await?;

        log::info!("[Snarkify Client], {method}, received response");
        log::debug!("[Snarkify Client], {method}, response: {response_body}");
        serde_json::from_str(&response_body).map_err(|e| anyhow::anyhow!(e))
    }
}
