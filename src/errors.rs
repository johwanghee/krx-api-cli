use anyhow::Error as AnyError;
use clap::error::ErrorKind;
use reqwest::Error as ReqwestError;
use reqwest::StatusCode;
use serde::Serialize;
use serde_json::Value;

use crate::config::PlaintextSecretError;

pub const PROGRAM_ERROR_EXIT_CODE: i32 = 2;
pub const API_ERROR_EXIT_CODE: i32 = 3;
const RESPONSE_EXCERPT_LIMIT: usize = 1_000;

#[derive(Debug)]
pub struct KrxApiError {
    pub operation: String,
    pub path: String,
    pub http_status: Option<u16>,
    pub code: Option<String>,
    pub message: Option<String>,
    pub response_excerpt: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorEnvelope {
    pub ok: bool,
    pub error_type: &'static str,
    pub exit_code: i32,
    pub message: String,
    pub llm_hint: LlmHint,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_error: Option<ApiErrorPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program_error: Option<ProgramErrorPayload>,
    pub causes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LlmHint {
    pub summary: String,
    pub retryable: bool,
    pub next_action: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiErrorPayload {
    pub category: &'static str,
    pub operation: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_excerpt: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProgramErrorPayload {
    pub category: &'static str,
    pub retryable: bool,
    pub detail: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub plaintext_secrets: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub suggested_commands: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub suggested_env_vars: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct ApiClassification {
    category: &'static str,
    retryable: bool,
    next_action: &'static str,
}

#[derive(Debug, Clone)]
struct ProgramClassification {
    category: &'static str,
    retryable: bool,
    next_action: String,
    suggested_commands: Vec<String>,
    suggested_env_vars: Vec<String>,
}

impl KrxApiError {
    pub fn from_http_response(
        operation: impl Into<String>,
        path: impl Into<String>,
        status: StatusCode,
        response_text: &str,
    ) -> Self {
        let (code, message) = parse_krx_error_fields(response_text);
        Self {
            operation: operation.into(),
            path: path.into(),
            http_status: Some(status.as_u16()),
            code,
            message,
            response_excerpt: Some(response_excerpt(response_text)),
        }
    }

    pub fn retryable(&self) -> bool {
        self.http_status
            .map(|status| status == 429 || status >= 500)
            .unwrap_or(false)
    }

    pub fn from_response_value(
        operation: impl Into<String>,
        path: impl Into<String>,
        http_status: Option<u16>,
        value: &Value,
    ) -> Self {
        let code = value
            .get("respCode")
            .and_then(Value::as_str)
            .or_else(|| value.get("code").and_then(Value::as_str))
            .map(ToString::to_string);
        let message = value
            .get("respMsg")
            .and_then(Value::as_str)
            .or_else(|| value.get("message").and_then(Value::as_str))
            .map(ToString::to_string);
        let response_excerpt = serde_json::to_string(value)
            .ok()
            .map(|text| response_excerpt(&text));

        Self {
            operation: operation.into(),
            path: path.into(),
            http_status,
            code,
            message,
            response_excerpt,
        }
    }

    pub fn invalid_json_response(
        operation: impl Into<String>,
        path: impl Into<String>,
        http_status: Option<u16>,
        response_text: &str,
    ) -> Self {
        Self {
            operation: operation.into(),
            path: path.into(),
            http_status,
            code: Some("invalid_json_response".to_string()),
            message: Some("KRX returned a non-JSON body while JSON was requested.".to_string()),
            response_excerpt: Some(response_excerpt(response_text)),
        }
    }
}

impl std::fmt::Display for KrxApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let (Some(code), Some(message)) = (&self.code, &self.message) {
            write!(
                f,
                "KRX API error {code} for {} {}: {message}",
                self.operation, self.path
            )
        } else if let Some(status) = self.http_status {
            write!(
                f,
                "KRX HTTP error {status} for {} {}",
                self.operation, self.path
            )
        } else {
            write!(f, "KRX API error for {} {}", self.operation, self.path)
        }
    }
}

impl std::error::Error for KrxApiError {}

pub fn error_report_from_anyhow(error: &AnyError) -> ErrorEnvelope {
    if let Some(api_error) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<KrxApiError>())
    {
        return api_error_report(api_error, error.chain().map(ToString::to_string).collect());
    }

    if let Some(plaintext_error) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<PlaintextSecretError>())
    {
        return plaintext_secret_report(
            plaintext_error,
            error.chain().map(ToString::to_string).collect(),
        );
    }

    let detail = error.to_string();
    let classification = classify_program_error(error, &detail);
    ErrorEnvelope {
        ok: false,
        error_type: "program_error",
        exit_code: PROGRAM_ERROR_EXIT_CODE,
        message: detail.clone(),
        llm_hint: LlmHint {
            summary: program_summary(classification.category, &detail),
            retryable: classification.retryable,
            next_action: classification.next_action.clone(),
        },
        api_error: None,
        program_error: Some(ProgramErrorPayload {
            category: classification.category,
            retryable: classification.retryable,
            detail,
            plaintext_secrets: Vec::new(),
            suggested_commands: classification.suggested_commands,
            suggested_env_vars: classification.suggested_env_vars,
        }),
        causes: error.chain().map(ToString::to_string).collect(),
    }
}

pub fn error_report_from_clap(error: &clap::Error) -> ErrorEnvelope {
    let rendered = error.to_string();
    let detail = rendered.trim().to_string();
    let is_help_or_version = matches!(
        error.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    );
    let next_action = if matches!(
        error.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    ) {
        "No action required."
    } else {
        "Read the command help shown in `detail`, then fix the CLI arguments and retry."
    };

    ErrorEnvelope {
        ok: false,
        error_type: "program_error",
        exit_code: PROGRAM_ERROR_EXIT_CODE,
        message: detail.clone(),
        llm_hint: LlmHint {
            summary: "The CLI arguments did not match the command definition.".to_string(),
            retryable: false,
            next_action: next_action.to_string(),
        },
        api_error: None,
        program_error: Some(ProgramErrorPayload {
            category: "invalid_input",
            retryable: false,
            detail,
            plaintext_secrets: Vec::new(),
            suggested_commands: if is_help_or_version {
                Vec::new()
            } else {
                vec![
                    "krx-api-cli --help".to_string(),
                    "krx-api-cli catalog summary".to_string(),
                ]
            },
            suggested_env_vars: Vec::new(),
        }),
        causes: vec![error.kind().to_string()],
    }
}

fn plaintext_secret_report(
    plaintext_error: &PlaintextSecretError,
    causes: Vec<String>,
) -> ErrorEnvelope {
    let suggested_commands = vec![
        "krx-api-cli config key status --compact".to_string(),
        "krx-api-cli config seal".to_string(),
    ];

    ErrorEnvelope {
        ok: false,
        error_type: "program_error",
        exit_code: PROGRAM_ERROR_EXIT_CODE,
        message: plaintext_error.to_string(),
        llm_hint: LlmHint {
            summary: format!(
                "Sensitive config values are still stored in plaintext in {}.",
                plaintext_error.config_path.display()
            ),
            retryable: false,
            next_action: "Run `krx-api-cli config key status --compact` to inspect plaintext fields, then run `krx-api-cli config seal` or `krx-api-cli config set-auth-key ...` to encrypt them.".to_string(),
        },
        api_error: None,
        program_error: Some(ProgramErrorPayload {
            category: "plaintext_secret_detected",
            retryable: false,
            detail: plaintext_error.to_string(),
            plaintext_secrets: plaintext_error.plaintext_fields.clone(),
            suggested_commands,
            suggested_env_vars: Vec::new(),
        }),
        causes,
    }
}

pub fn render_error_report(report: &ErrorEnvelope, compact: bool) -> String {
    let rendered = if compact {
        serde_json::to_string(report)
    } else {
        serde_json::to_string_pretty(report)
    };

    rendered.unwrap_or_else(|serialization_error| {
        format!(
            "{{\"ok\":false,\"error_type\":\"program_error\",\"exit_code\":{PROGRAM_ERROR_EXIT_CODE},\"message\":\"failed to serialize error report: {serialization_error}\"}}"
        )
    })
}

fn api_error_report(api_error: &KrxApiError, causes: Vec<String>) -> ErrorEnvelope {
    let classification = classify_api_error(api_error);
    let summary = if let (Some(code), Some(message)) = (&api_error.code, &api_error.message) {
        format!(
            "KRX rejected the request for {} with code={} and message={}.",
            api_error.path, code, message
        )
    } else if let Some(status) = api_error.http_status {
        format!(
            "KRX returned HTTP {} for {} {}.",
            status, api_error.operation, api_error.path
        )
    } else {
        format!(
            "KRX returned an API error for {} {}.",
            api_error.operation, api_error.path
        )
    };

    ErrorEnvelope {
        ok: false,
        error_type: "api_error",
        exit_code: API_ERROR_EXIT_CODE,
        message: api_error.to_string(),
        llm_hint: LlmHint {
            summary,
            retryable: classification.retryable,
            next_action: classification.next_action.to_string(),
        },
        api_error: Some(ApiErrorPayload {
            category: classification.category,
            operation: api_error.operation.clone(),
            path: api_error.path.clone(),
            http_status: api_error.http_status,
            code: api_error.code.clone(),
            message: api_error.message.clone(),
            response_excerpt: api_error.response_excerpt.clone(),
        }),
        program_error: None,
        causes,
    }
}

fn parse_krx_error_fields(response_text: &str) -> (Option<String>, Option<String>) {
    let parsed: Value = match serde_json::from_str(response_text) {
        Ok(value) => value,
        Err(_) => return (None, None),
    };

    let code = parsed
        .get("respCode")
        .and_then(Value::as_str)
        .or_else(|| parsed.get("code").and_then(Value::as_str))
        .map(ToString::to_string);
    let message = parsed
        .get("respMsg")
        .and_then(Value::as_str)
        .or_else(|| parsed.get("message").and_then(Value::as_str))
        .map(ToString::to_string);

    (code, message)
}

fn classify_api_error(api_error: &KrxApiError) -> ApiClassification {
    if api_error.code.as_deref() == Some("invalid_json_response") {
        return ApiClassification {
            category: "invalid_response_format",
            retryable: false,
            next_action:
                "Retry with `--format xml` to inspect the raw upstream payload, then adjust parsing or report the upstream issue.",
        };
    }

    let response_text = format!(
        "{} {} {}",
        api_error.code.as_deref().unwrap_or_default(),
        api_error.message.as_deref().unwrap_or_default(),
        api_error.response_excerpt.as_deref().unwrap_or_default(),
    );
    let response_text = response_text.to_ascii_lowercase();

    if matches!(api_error.http_status, Some(401) | Some(403))
        || contains_any(
            &response_text,
            &[
                "auth_key",
                "authorization",
                "unauthorized",
                "forbidden",
                "permission",
                "access denied",
                "인증",
                "권한",
            ],
        )
    {
        return ApiClassification {
            category: "auth_or_permission",
            retryable: false,
            next_action:
                "Verify AUTH_KEY, environment approval, and endpoint access permission, then retry.",
        };
    }

    if matches!(api_error.http_status, Some(404))
        || contains_any(
            &response_text,
            &["not found", "unknown api", "없는 api", "존재하지 않는"],
        )
    {
        return ApiClassification {
            category: "endpoint_not_found",
            retryable: false,
            next_action:
                "Verify the API path and refresh the local service catalog/spec docs before retrying.",
        };
    }

    if matches!(api_error.http_status, Some(429))
        || contains_any(
            &response_text,
            &[
                "too many",
                "rate limit",
                "throttle",
                "호출 제한",
                "요청 제한",
            ],
        )
    {
        return ApiClassification {
            category: "rate_limited",
            retryable: true,
            next_action: "Wait briefly and retry the same request.",
        };
    }

    match api_error.http_status {
        Some(status) if status >= 500 => ApiClassification {
            category: "upstream_failure",
            retryable: true,
            next_action: "Retry after a short delay; if it persists, treat it as an upstream outage.",
        },
        Some(status) if status >= 400 => ApiClassification {
            category: "request_rejected",
            retryable: false,
            next_action:
                "Verify AUTH_KEY, request parameters, and whether the API is enabled for your account.",
        },
        _ => ApiClassification {
            category: "api_reported_error",
            retryable: api_error.retryable(),
            next_action:
                "Inspect `api_error.code`, `api_error.message`, and `response_excerpt`, then adjust the request before retrying.",
        },
    }
}

fn classify_program_error(error: &AnyError, detail: &str) -> ProgramClassification {
    if detail.contains("missing AUTH_KEY") {
        return ProgramClassification {
            category: "missing_auth_key",
            retryable: false,
            next_action:
                "Set AUTH_KEY in config or environment for the selected profile, then retry."
                    .to_string(),
            suggested_commands: vec![
                "krx-api-cli config set-auth-key --profile sample --stdin".to_string(),
                "krx-api-cli config set-auth-key --profile real --stdin".to_string(),
            ],
            suggested_env_vars: vec![
                "KRX_SAMPLE_AUTH_KEY".to_string(),
                "KRX_REAL_AUTH_KEY".to_string(),
                "KRX_AUTH_KEY".to_string(),
            ],
        };
    }

    if detail.contains("`basDd` must be YYYYMMDD")
        || detail.contains("missing required parameter")
        || detail.contains("unknown API command")
        || detail.contains("unknown category")
        || detail.contains("unknown subcommand")
        || detail.contains("invalid value")
        || detail.contains("provide either --value or --stdin")
        || detail.contains("auth key cannot be empty")
        || detail.contains("stdin secret was empty")
        || detail.contains("unsupported format")
        || detail.contains("unsupported environment")
        || detail.contains("unsupported sort order")
        || detail.contains("invalid --filter expression")
        || detail.contains("invalid --filter operator")
        || detail.contains("select field list cannot be empty")
        || detail.contains("limit must be greater than zero")
        || detail.contains("response transforms require")
        || detail.contains("unknown response field")
        || detail.contains("unknown response field or alias")
        || detail.contains("response field alias")
    {
        return ProgramClassification {
            category: "invalid_input",
            retryable: false,
            next_action: "Fix the CLI arguments or input values, then retry.".to_string(),
            suggested_commands: vec![
                "krx-api-cli --help".to_string(),
                "krx-api-cli catalog summary".to_string(),
            ],
            suggested_env_vars: Vec::new(),
        };
    }

    if detail.contains("failed to read config file")
        || detail.contains("failed to parse config file")
        || detail.contains("failed to write config")
        || detail.contains("config already exists")
        || detail.contains("config file does not exist")
        || detail.contains("failed to read config key file")
        || detail.contains("failed to parse config key file")
        || detail.contains("failed to write config encryption key")
        || detail.contains("failed to decrypt")
        || detail.contains("failed to encrypt")
        || detail.contains("config encryption key")
        || detail.contains("encrypted config")
        || detail.contains("failed to create key directory")
        || detail.contains("invalid config key length")
        || detail.contains("unsupported config key file version")
        || detail.contains("failed to apply restrictive permissions")
    {
        return ProgramClassification {
            category: "config_error",
            retryable: false,
            next_action:
                "Inspect the config path, fix the file contents or permissions, then retry."
                    .to_string(),
            suggested_commands: vec![
                "krx-api-cli config path".to_string(),
                "krx-api-cli config show".to_string(),
            ],
            suggested_env_vars: vec!["KRX_CONFIG".to_string()],
        };
    }

    if let Some(reqwest_error) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<ReqwestError>())
    {
        if reqwest_error.is_timeout() {
            return ProgramClassification {
                category: "network_timeout",
                retryable: true,
                next_action: "Retry the same request after a short delay.".to_string(),
                suggested_commands: Vec::new(),
                suggested_env_vars: Vec::new(),
            };
        }

        if reqwest_error.is_connect() {
            return ProgramClassification {
                category: "network_connectivity",
                retryable: true,
                next_action: "Check network connectivity and DNS/TLS reachability, then retry."
                    .to_string(),
                suggested_commands: Vec::new(),
                suggested_env_vars: Vec::new(),
            };
        }
    }

    if detail.contains("failed to build HTTP client") {
        return ProgramClassification {
            category: "client_init_failure",
            retryable: false,
            next_action: "Inspect the local runtime and TLS/client configuration, then retry."
                .to_string(),
            suggested_commands: Vec::new(),
            suggested_env_vars: Vec::new(),
        };
    }

    ProgramClassification {
        category: "runtime_failure",
        retryable: false,
        next_action: "Inspect the error detail, fix the local problem, then retry.".to_string(),
        suggested_commands: Vec::new(),
        suggested_env_vars: Vec::new(),
    }
}

fn program_summary(category: &str, detail: &str) -> String {
    match category {
        "missing_auth_key" => {
            "The CLI could not find an AUTH_KEY for the selected environment.".to_string()
        }
        "plaintext_secret_detected" => {
            "The CLI found plaintext AUTH_KEY values in the local config and refused to use them."
                .to_string()
        }
        "invalid_input" => {
            "The CLI arguments or parameter values did not match the command requirements."
                .to_string()
        }
        "config_error" => {
            "The CLI could not read or write the local configuration correctly.".to_string()
        }
        "network_timeout" => "The request failed because the network timed out.".to_string(),
        "network_connectivity" => {
            "The request failed before reaching KRX because of a connectivity problem.".to_string()
        }
        "client_init_failure" => "The CLI could not initialize its HTTP client.".to_string(),
        _ => format!("The CLI could not complete the request because of a local problem: {detail}"),
    }
}

fn response_excerpt(response_text: &str) -> String {
    response_text.chars().take(RESPONSE_EXCERPT_LIMIT).collect()
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn plaintext_secret_error_is_structured_for_llm_remediation() {
        let report = error_report_from_anyhow(&AnyError::new(PlaintextSecretError {
            config_path: PathBuf::from("/tmp/config.toml"),
            plaintext_fields: vec![
                "profiles.sample.auth_key".to_string(),
                "profiles.real.auth_key".to_string(),
            ],
        }));

        assert_eq!(report.error_type, "program_error");
        assert_eq!(
            report
                .program_error
                .as_ref()
                .map(|payload| payload.category),
            Some("plaintext_secret_detected")
        );
        assert_eq!(
            report
                .program_error
                .as_ref()
                .map(|payload| payload.plaintext_secrets.len()),
            Some(2)
        );
    }

    #[test]
    fn invalid_input_keeps_plaintext_secret_list_empty() {
        let report = error_report_from_anyhow(&anyhow!("invalid --filter expression `broken`"));
        assert_eq!(
            report
                .program_error
                .as_ref()
                .map(|payload| payload.plaintext_secrets.is_empty()),
            Some(true)
        );
    }
}
