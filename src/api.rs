use anyhow::{Context, Result};
use clap::ValueEnum;
use reqwest::blocking::Client;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::ResolvedProfile;
use crate::errors::KrxApiError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Json,
    Xml,
}

impl OutputFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Xml => "xml",
        }
    }

    fn accept(self) -> &'static str {
        match self {
            Self::Json => "application/json",
            Self::Xml => "application/xml",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ApiRequest {
    pub path: String,
    pub query: Vec<(String, String)>,
    pub format: OutputFormat,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiResponse {
    Json(Value),
    Xml(String),
}

pub struct KrxClient {
    http: Client,
    profile: ResolvedProfile,
}

impl KrxClient {
    pub fn new(profile: ResolvedProfile) -> Result<Self> {
        let http = Client::builder()
            .user_agent(profile.user_agent.clone())
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self { http, profile })
    }

    pub fn send_request(&self, request: ApiRequest) -> Result<ApiResponse> {
        let url = build_url(&self.profile.base_url, &request.path, request.format);
        let response = self
            .http
            .get(&url)
            .header("AUTH_KEY", &self.profile.auth_key)
            .header("Accept", request.format.accept())
            .query(&request.query)
            .send()
            .context("failed to execute KRX request")?;

        parse_api_response(
            response.status(),
            response.text().context("failed to read response body")?,
            &request,
        )
    }
}

fn build_url(base_url: &str, path: &str, format: OutputFormat) -> String {
    format!(
        "{}{}.{}",
        base_url.trim_end_matches('/'),
        path,
        format.extension()
    )
}

fn parse_api_response(
    status: StatusCode,
    text: String,
    request: &ApiRequest,
) -> Result<ApiResponse> {
    if !status.is_success() {
        return Err(
            KrxApiError::from_http_response("api_call", &request.path, status, &text).into(),
        );
    }

    match request.format {
        OutputFormat::Json => {
            let value: Value = serde_json::from_str(&text).map_err(|_| {
                KrxApiError::invalid_json_response(
                    "api_call",
                    &request.path,
                    Some(status.as_u16()),
                    &text,
                )
            })?;

            if response_value_looks_like_error(&value) {
                return Err(KrxApiError::from_response_value(
                    "api_call",
                    &request.path,
                    Some(status.as_u16()),
                    &value,
                )
                .into());
            }

            Ok(ApiResponse::Json(value))
        }
        OutputFormat::Xml => Ok(ApiResponse::Xml(text)),
    }
}

fn response_value_looks_like_error(value: &Value) -> bool {
    let code = value
        .get("respCode")
        .and_then(Value::as_str)
        .or_else(|| value.get("code").and_then(Value::as_str));

    match code {
        Some("0") | Some("200") | Some("0000") => false,
        Some(_) => true,
        None => false,
    }
}
