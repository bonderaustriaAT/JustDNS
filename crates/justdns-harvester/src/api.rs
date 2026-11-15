// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The read-only client for the JustDNS HTTP API.
//!
//! The query log is only reachable over the API (the DuckDB file is single-writer
//! locked), and the API is guarded by a session cookie — there is no API key for
//! the history endpoint. So the harvester logs in as a dedicated **Viewer**
//! service account (`POST /api/auth/login`), keeps the `justdns_session` cookie,
//! and pages `GET /api/queries/history` newest-first until it reaches rows it has
//! already seen. Viewer is enough: reads need only Viewer, and the account can do
//! nothing else. The account must have MFA disabled, since this flow has no second
//! step. The password is read from the environment and never logged.

use serde::Deserialize;
use tracing::{debug, info, warn};

use crate::model::{HistoryPage, LogRow};

/// The session cookie JustDNS sets on login.
const SESSION_COOKIE: &str = "justdns_session";

/// A read-only client bound to one instance and one service account.
pub struct ApiClient {
    agent: ureq::Agent,
    base: String,
    username: String,
    password: String,
    cookie: Option<String>,
}

#[derive(Deserialize)]
struct LoginResponse {
    #[serde(default)]
    status: String,
}

impl ApiClient {
    /// A client for `base` (e.g. `https://dns.example`) and the service account.
    pub fn new(
        base: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
        agent: ureq::Agent,
    ) -> Self {
        Self {
            agent,
            base: base.into().trim_end_matches('/').to_owned(),
            username: username.into(),
            password: password.into(),
            cookie: None,
        }
    }

    /// Log in and capture the session cookie. Errors if the account needs a
    /// second step (MFA/enroll) — a service account must not.
    pub fn login(&mut self) -> Result<(), ApiError> {
        let url = format!("{}/api/auth/login", self.base);
        let body = serde_json::json!({
            "username": self.username,
            "password": self.password,
        })
        .to_string();
        let response = self
            .agent
            .post(&url)
            .set("Content-Type", "application/json")
            .send_string(&body)
            .map_err(ApiError::from_ureq)?;

        let cookie = response.all("set-cookie").into_iter().find_map(|value| {
            value
                .split(';')
                .next()
                .filter(|pair| pair.trim_start().starts_with(&format!("{SESSION_COOKIE}=")))
                .map(str::to_owned)
        });

        let text = response
            .into_string()
            .map_err(|e| ApiError::Decode(e.to_string()))?;
        let parsed: LoginResponse =
            serde_json::from_str(&text).map_err(|e| ApiError::Decode(e.to_string()))?;
        if parsed.status != "authenticated" {
            return Err(ApiError::NeedsSecondStep(parsed.status));
        }
        match cookie {
            Some(cookie) => {
                self.cookie = Some(cookie);
                info!(user = %self.username, "logged in to JustDNS API");
                Ok(())
            }
            None => Err(ApiError::NoCookie),
        }
    }

    /// GET a path with the session cookie, re-logging in once on a 401.
    fn get(&mut self, path: &str) -> Result<ureq::Response, ApiError> {
        if self.cookie.is_none() {
            self.login()?;
        }
        let url = format!("{}{path}", self.base);
        let attempt = |client: &ApiClient| {
            let mut req = client.agent.get(&url);
            if let Some(cookie) = &client.cookie {
                req = req.set("Cookie", cookie);
            }
            req.call()
        };
        match attempt(self) {
            Ok(response) => Ok(response),
            Err(ureq::Error::Status(401, _)) => {
                debug!("session expired, re-authenticating");
                self.cookie = None;
                self.login()?;
                attempt(self).map_err(ApiError::from_ureq)
            }
            Err(error) => Err(ApiError::from_ureq(error)),
        }
    }

    /// Pull every log row newer than `since_ms` for one source, paging
    /// newest-first until the rows fall at or below the cursor. `response_code`
    /// filters to one rcode (e.g. `Some(0)` = NOERROR for the resolved feed);
    /// `None` leaves it unfiltered (the blocked feed). Bounded by `max_pages` so a
    /// first run on a huge backlog cannot spin forever.
    pub fn poll_since(
        &mut self,
        source: &str,
        response_code: Option<u16>,
        since_ms: i64,
        page_limit: usize,
        max_pages: usize,
    ) -> Result<Vec<LogRow>, ApiError> {
        let mut collected = Vec::new();
        let mut cursor: Option<(i64, i64)> = None;
        for _ in 0..max_pages {
            let mut path = format!("/api/queries/history?source={source}&limit={page_limit}");
            if let Some(rcode) = response_code {
                path.push_str(&format!("&responseCode={rcode}"));
            }
            if let Some((before_ms, before_row)) = cursor {
                path.push_str(&format!("&beforeMs={before_ms}&beforeRow={before_row}"));
            }
            let text = self
                .get(&path)?
                .into_string()
                .map_err(|e| ApiError::Decode(e.to_string()))?;
            let page: HistoryPage =
                serde_json::from_str(&text).map_err(|e| ApiError::Decode(e.to_string()))?;

            let mut reached_old = false;
            for row in &page.rows {
                if row.at_ms > since_ms {
                    collected.push(row.clone());
                } else {
                    reached_old = true;
                }
            }
            if reached_old {
                break;
            }
            match page.next_before {
                Some(next) => cursor = Some((next.at_ms, next.row)),
                None => break,
            }
        }
        debug!(source, rows = collected.len(), "polled history page set");
        if collected.len() >= page_limit * max_pages {
            warn!(
                source,
                "hit the page cap; some older rows may be unread this run"
            );
        }
        Ok(collected)
    }
}

/// Errors talking to the API.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("http {0}")]
    Http(u16),
    #[error("transport: {0}")]
    Transport(String),
    #[error("decode: {0}")]
    Decode(String),
    #[error("login needs a second step (status: {0}) — the service account must have MFA disabled")]
    NeedsSecondStep(String),
    #[error("login returned no session cookie")]
    NoCookie,
}

impl ApiError {
    fn from_ureq(error: ureq::Error) -> Self {
        if let ureq::Error::Status(code, _) = error {
            ApiError::Http(code)
        } else {
            ApiError::Transport(error.to_string())
        }
    }
}
