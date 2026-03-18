mod api;
mod cli;
mod config;
mod errors;
mod manifest;

use std::cmp::Ordering;
use std::path::Path;
use std::{io, io::Read};

use anyhow::{anyhow, Context};
use clap::error::ErrorKind;
use clap::{value_parser, Arg, ArgAction, ArgMatches, Command};
use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::api::{ApiRequest, ApiResponse, KrxClient, OutputFormat};
use crate::cli::Environment;
use crate::config::{
    app_paths, key_status, redacted_config_value, resolve_profile, seal_config, set_auth_key,
    write_config_template,
};
use crate::errors::{
    error_report_from_anyhow, error_report_from_clap, render_error_report, API_ERROR_EXIT_CODE,
    PROGRAM_ERROR_EXIT_CODE,
};
use crate::manifest::{load_manifest, ApiEntry, ApiManifest, ApiParam};

fn main() {
    let compact = requested_compact_output();

    match run() {
        Ok(()) => {}
        Err(RunFailure::Clap(error))
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            let _ = error.print();
            std::process::exit(0);
        }
        Err(RunFailure::Clap(error)) => {
            eprintln!(
                "{}",
                render_error_report(&error_report_from_clap(&error), compact)
            );
            std::process::exit(PROGRAM_ERROR_EXIT_CODE);
        }
        Err(RunFailure::Runtime(error)) => {
            let report = error_report_from_anyhow(&error);
            eprintln!("{}", render_error_report(&report, compact));
            std::process::exit(match report.error_type {
                "api_error" => API_ERROR_EXIT_CODE,
                _ => PROGRAM_ERROR_EXIT_CODE,
            });
        }
    }
}

enum RunFailure {
    Clap(clap::Error),
    Runtime(anyhow::Error),
}

#[derive(Debug, Clone, Copy)]
struct FieldAlias {
    alias: &'static str,
    candidates: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, Default)]
enum SortOrder {
    #[default]
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy)]
enum FilterOperator {
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
    Contains,
}

#[derive(Debug, Clone, Default)]
struct ResponseTransform {
    filters: Vec<FilterExpr>,
    sort_by: Option<String>,
    sort_order: SortOrder,
    limit: Option<usize>,
    select: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
struct ResolvedField {
    output_key: String,
    source_key: String,
}

#[derive(Debug, Clone)]
struct FilterExpr {
    field: String,
    operator: FilterOperator,
    value: String,
}

#[derive(Debug, Clone)]
struct ResolvedFilter {
    source_key: String,
    operator: FilterOperator,
    value: String,
}

const FIELD_ALIASES: &[FieldAlias] = &[
    FieldAlias {
        alias: "date",
        candidates: &["BAS_DD"],
    },
    FieldAlias {
        alias: "name",
        candidates: &["ISU_NM", "IDX_NM"],
    },
    FieldAlias {
        alias: "symbol",
        candidates: &["ISU_CD"],
    },
    FieldAlias {
        alias: "market",
        candidates: &["MKT_NM", "IDX_CLSS"],
    },
    FieldAlias {
        alias: "market_cap",
        candidates: &["MKTCAP"],
    },
    FieldAlias {
        alias: "close_price",
        candidates: &["TDD_CLSPRC", "CLSPRC_IDX"],
    },
    FieldAlias {
        alias: "open_price",
        candidates: &["TDD_OPNPRC", "OPNPRC_IDX"],
    },
    FieldAlias {
        alias: "high_price",
        candidates: &["TDD_HGPRC", "HGPRC_IDX"],
    },
    FieldAlias {
        alias: "low_price",
        candidates: &["TDD_LWPRC", "LWPRC_IDX"],
    },
    FieldAlias {
        alias: "change_price",
        candidates: &["CMPPREVDD_PRC", "CMPPREVDD_IDX"],
    },
    FieldAlias {
        alias: "change_rate",
        candidates: &["FLUC_RT"],
    },
    FieldAlias {
        alias: "volume",
        candidates: &["ACC_TRDVOL"],
    },
    FieldAlias {
        alias: "value",
        candidates: &["ACC_TRDVAL"],
    },
    FieldAlias {
        alias: "listed_shares",
        candidates: &["LIST_SHRS"],
    },
];

impl From<clap::Error> for RunFailure {
    fn from(value: clap::Error) -> Self {
        Self::Clap(value)
    }
}

impl From<anyhow::Error> for RunFailure {
    fn from(value: anyhow::Error) -> Self {
        Self::Runtime(value)
    }
}

fn run() -> std::result::Result<(), RunFailure> {
    let manifest = load_manifest()?;
    let matches = build_cli(manifest).try_get_matches()?;
    let env = environment_from_matches(&matches)?;
    let config_path = matches.get_one::<String>("config").map(String::as_str);
    let compact = matches.get_flag("compact");

    match matches.subcommand() {
        Some(("doctor", _)) => Ok(handle_doctor(manifest, config_path, env, compact)?),
        Some(("config", sub_matches)) => Ok(handle_config(sub_matches, config_path, compact)?),
        Some(("catalog", sub_matches)) => Ok(handle_catalog(manifest, sub_matches, compact)?),
        Some((category_name, category_matches)) => {
            let category = manifest
                .category_by_name(category_name)
                .ok_or_else(|| anyhow!("unknown category `{category_name}`"))?;
            let (api_name, api_matches) = category_matches
                .subcommand()
                .ok_or_else(|| anyhow!("missing API command under category `{}`", category.id))?;

            let entry = manifest
                .entry_by_command(&category.id, api_name)
                .ok_or_else(|| {
                    anyhow!("unknown API command `{api_name}` under `{}`", category.id)
                })?;

            let format = output_format_from_matches(&matches)?;
            let request = build_manifest_request(entry, api_matches, format, env)?;
            let transform = response_transform_from_matches(api_matches)?;
            let client = build_client(config_path.map(Path::new), env)?;
            let payload = client.send_request(request)?;
            let payload = apply_response_transform(payload, &transform)?;
            Ok(print_response(&payload, compact)?)
        }
        None => Err(anyhow!("no command provided").into()),
    }
}

fn requested_compact_output() -> bool {
    std::env::args_os().any(|argument| argument == "--compact")
}

fn build_cli(manifest: &ApiManifest) -> Command {
    let mut command = Command::new("krx-api-cli")
        .about("Manifest-driven CLI for KRX Open API")
        .version(env!("CARGO_PKG_VERSION"))
        .subcommand_required(true)
        .arg_required_else_help(true)
        .after_help(top_level_after_help(manifest))
        .arg(global_env_arg())
        .arg(global_config_arg())
        .arg(global_output_format_arg())
        .arg(global_compact_arg())
        .subcommand(doctor_command())
        .subcommand(config_command())
        .subcommand(catalog_command());

    for category in &manifest.categories {
        let mut category_command = Command::new(leak_string(category.id.clone()))
            .about(category.description.clone())
            .long_about(category_long_about(category))
            .subcommand_required(true)
            .arg_required_else_help(true);

        for entry in manifest.category_entries(&category.id) {
            let mut api_command = Command::new(leak_string(entry.command_name.clone()))
                .about(entry.display_name.clone())
                .long_about(api_long_about(entry))
                .after_help(api_transform_after_help());

            for param in &entry.params {
                api_command = api_command.arg(api_arg(param));
            }

            api_command = api_command
                .arg(transform_filter_arg())
                .arg(transform_sort_by_arg())
                .arg(transform_order_arg())
                .arg(transform_limit_arg())
                .arg(transform_select_arg());

            category_command = category_command.subcommand(api_command);
        }

        command = command.subcommand(category_command);
    }

    command
}

fn global_env_arg() -> Arg {
    Arg::new("env")
        .long("env")
        .global(true)
        .env("KRX_ENV")
        .default_value("sample")
        .value_parser(["sample", "real"])
        .help("KRX environment")
}

fn global_config_arg() -> Arg {
    Arg::new("config")
        .long("config")
        .global(true)
        .env("KRX_CONFIG")
        .help("Override config file path")
        .value_name("PATH")
}

fn global_output_format_arg() -> Arg {
    Arg::new("format")
        .long("format")
        .global(true)
        .default_value("json")
        .value_parser(["json", "xml"])
        .help("Response format")
}

fn global_compact_arg() -> Arg {
    Arg::new("compact")
        .long("compact")
        .global(true)
        .action(ArgAction::SetTrue)
        .help("Print compact JSON")
}

fn transform_sort_by_arg() -> Arg {
    Arg::new("sort_by")
        .long("sort-by")
        .value_name("FIELD")
        .help("Sort list responses by a response field")
}

fn transform_filter_arg() -> Arg {
    Arg::new("filter")
        .long("filter")
        .value_name("FIELD:OP:VALUE")
        .action(ArgAction::Append)
        .help("Filter rows; repeatable. Ops: eq, ne, gt, gte, lt, lte, contains")
}

fn transform_order_arg() -> Arg {
    Arg::new("order")
        .long("order")
        .value_name("ORDER")
        .default_value("asc")
        .value_parser(["asc", "desc"])
        .requires("sort_by")
        .help("Sort order for --sort-by")
}

fn transform_limit_arg() -> Arg {
    Arg::new("limit")
        .long("limit")
        .value_name("N")
        .value_parser(value_parser!(usize))
        .help("Keep only the first N rows after sorting")
}

fn transform_select_arg() -> Arg {
    Arg::new("select")
        .long("select")
        .value_name("FIELDS")
        .help("Comma-separated response fields to keep in each row")
}

fn config_command() -> Command {
    Command::new("config")
        .about("Manage local CLI configuration")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("init").about("Write a config template").arg(
                Arg::new("force")
                    .long("force")
                    .action(ArgAction::SetTrue)
                    .help("Overwrite an existing config file"),
            ),
        )
        .subcommand(Command::new("path").about("Show config and key paths"))
        .subcommand(Command::new("show").about("Show redacted config"))
        .subcommand(
            Command::new("seal")
                .about("Encrypt plaintext AUTH_KEY values already stored in config")
                .arg(
                    Arg::new("profile")
                        .long("profile")
                        .value_parser(["sample", "real"])
                        .help("Only seal a single profile"),
                ),
        )
        .subcommand(
            Command::new("key")
                .about("Inspect local config encryption key state")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .subcommand(
                    Command::new("status")
                        .about("Show key status, plaintext secret status, and remediation hints"),
                ),
        )
        .subcommand(
            Command::new("set-auth-key")
                .about("Encrypt and store AUTH_KEY in config")
                .arg(
                    Arg::new("profile")
                        .long("profile")
                        .required(true)
                        .value_parser(["sample", "real"])
                        .help("Config profile to update"),
                )
                .arg(
                    Arg::new("value")
                        .long("value")
                        .value_name("VALUE")
                        .conflicts_with("stdin")
                        .help("AUTH_KEY value to store"),
                )
                .arg(
                    Arg::new("stdin")
                        .long("stdin")
                        .action(ArgAction::SetTrue)
                        .conflicts_with("value")
                        .help("Read AUTH_KEY from stdin"),
                ),
        )
}

fn doctor_command() -> Command {
    Command::new("doctor").about(
        "Inspect local CLI readiness, config encryption state, and selected profile resolution",
    )
}

fn catalog_command() -> Command {
    Command::new("catalog")
        .about("Inspect the embedded API catalog")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(Command::new("summary").about("Show category and API counts"))
        .subcommand(Command::new("export").about("Export the embedded manifest JSON"))
}

fn api_arg(param: &ApiParam) -> Arg {
    Arg::new(leak_string(param.name.clone()))
        .long(leak_string(param.cli_name.clone()))
        .required(param.required)
        .value_name(leak_string(param.name.to_uppercase()))
        .help(param.description.clone())
}

fn handle_config(
    sub_matches: &ArgMatches,
    config_path: Option<&str>,
    compact: bool,
) -> anyhow::Result<()> {
    match sub_matches.subcommand() {
        Some(("init", init_matches)) => {
            let paths = app_paths(config_path.map(Path::new))?;
            write_config_template(&paths.config_path, init_matches.get_flag("force"))?;
            print_json(
                &json!({
                    "ok": true,
                    "config_path": paths.config_path,
                }),
                compact,
            )
        }
        Some(("path", _)) => {
            let paths = app_paths(config_path.map(Path::new))?;
            print_json(
                &json!({
                    "config_path": paths.config_path,
                    "exists": paths.config_path.exists(),
                    "key_path": paths.key_path,
                    "key_exists": paths.key_path.exists(),
                }),
                compact,
            )
        }
        Some(("show", _)) => {
            let value = redacted_config_value(config_path.map(Path::new))?;
            print_json(&value, compact)
        }
        Some(("seal", seal_matches)) => {
            let environment = seal_matches
                .get_one::<String>("profile")
                .map(|value| environment_from_str(value))
                .transpose()?;
            let result = seal_config(config_path.map(Path::new), environment)?;
            print_json(
                &json!({
                    "encrypted_fields": result.encrypted_fields,
                    "profiles_touched": result.profiles_touched,
                    "config_path": result.config_path,
                    "key_path": result.key_path,
                }),
                compact,
            )
        }
        Some(("key", key_matches)) => handle_config_key(key_matches, config_path, compact),
        Some(("set-auth-key", set_matches)) => {
            let environment = environment_from_str(
                set_matches
                    .get_one::<String>("profile")
                    .map(String::as_str)
                    .unwrap_or("real"),
            )?;

            let value = match (
                set_matches.get_one::<String>("value").map(String::as_str),
                set_matches.get_flag("stdin"),
            ) {
                (Some(value), false) => value.to_string(),
                (None, true) => read_stdin_secret()?,
                _ => return Err(anyhow!("provide either --value or --stdin")),
            };

            let result = set_auth_key(config_path.map(Path::new), environment, &value)?;
            print_json(
                &json!({
                    "ok": true,
                    "profile": result.profile.as_str(),
                    "stored": "encrypted",
                    "config_path": result.config_path,
                    "key_path": result.key_path,
                }),
                compact,
            )
        }
        _ => Err(anyhow!("unknown config subcommand")),
    }
}

fn handle_config_key(
    matches: &ArgMatches,
    config_path: Option<&str>,
    compact: bool,
) -> anyhow::Result<()> {
    match matches.subcommand() {
        Some(("status", _)) => {
            let result = key_status(config_path.map(Path::new))?;
            print_json(
                &json!({
                    "key_path": result.key_path,
                    "key_exists": result.key_exists,
                    "key_format": result.key_format,
                    "previous_key_count": result.previous_key_count,
                    "encrypted_field_count": result.encrypted_field_count,
                    "plaintext_field_count": result.plaintext_field_count,
                    "plaintext_fields": result.plaintext_fields,
                    "seal_required": result.seal_required,
                    "suggested_commands": result.suggested_commands,
                }),
                compact,
            )
        }
        _ => Err(anyhow!("unknown config key subcommand")),
    }
}

fn handle_catalog(
    manifest: &ApiManifest,
    sub_matches: &ArgMatches,
    compact: bool,
) -> anyhow::Result<()> {
    match sub_matches.subcommand() {
        Some(("summary", _)) => print_json(
            &json!({
                "generated_at": manifest.generated_at,
                "category_count": manifest.category_count,
                "api_count": manifest.api_count,
                "categories": manifest.category_counts(),
            }),
            compact,
        ),
        Some(("export", _)) => print_json(manifest, compact),
        _ => Err(anyhow!("unknown catalog subcommand")),
    }
}

fn handle_doctor(
    manifest: &ApiManifest,
    config_path: Option<&str>,
    environment: Environment,
    compact: bool,
) -> anyhow::Result<()> {
    let config_snapshot = redacted_config_value(config_path.map(Path::new))?;
    let key_state = key_status(config_path.map(Path::new))?;
    let config_exists = config_snapshot
        .get("exists")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let encrypted_field_count = config_snapshot
        .get("encrypted_field_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let selected_profile = config_snapshot
        .get("profiles")
        .and_then(|profiles| profiles.get(environment.as_str()))
        .unwrap_or(&Value::Null);
    let auth_storage = selected_profile
        .get("auth_key")
        .and_then(|value| value.get("storage"))
        .and_then(Value::as_str)
        .unwrap_or("absent");
    let has_profile_base_url = selected_profile
        .get("base_url")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    let has_config_user_agent = config_snapshot
        .get("user_agent")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());

    let scoped_auth_env = env_var_is_set(&scoped_env_name(environment, "AUTH_KEY"));
    let global_auth_env = env_var_is_set("KRX_AUTH_KEY");
    let auth_key_source = if scoped_auth_env {
        "env_scoped"
    } else if global_auth_env {
        "env_global"
    } else {
        match auth_storage {
            "encrypted" => "config_encrypted",
            "plaintext" => "config_plaintext",
            _ => "missing",
        }
    };

    let base_url_source = if env_var_is_set(&scoped_env_name(environment, "BASE_URL")) {
        "env_scoped"
    } else if env_var_is_set("KRX_BASE_URL") {
        "env_global"
    } else if has_profile_base_url {
        "config"
    } else {
        "default"
    };

    let user_agent_source = if env_var_is_set("KRX_USER_AGENT") {
        "env_global"
    } else if has_config_user_agent {
        "config"
    } else {
        "default"
    };

    let auth_key_ready = !key_state.seal_required
        && matches!(
            auth_key_source,
            "env_scoped" | "env_global" | "config_encrypted"
        );

    let checks = vec![
        json!({
            "name": "config_file",
            "status": if config_exists { "ok" } else { "warn" },
            "detail": if config_exists {
                "Config file exists.".to_string()
            } else {
                "Config file does not exist. Env-only mode is still possible, but config-based AUTH_KEY storage is unavailable until `krx-api-cli config init`.".to_string()
            },
        }),
        json!({
            "name": "key_file",
            "status": if key_state.key_exists {
                "ok"
            } else if encrypted_field_count > 0 {
                "error"
            } else {
                "warn"
            },
            "detail": if key_state.key_exists {
                format!("Config encryption key exists at {}.", key_state.key_path.display())
            } else if encrypted_field_count > 0 {
                format!("Encrypted config fields exist but the key file is missing at {}.", key_state.key_path.display())
            } else {
                format!("No config encryption key exists yet at {}. This is expected until an AUTH_KEY is stored in config.", key_state.key_path.display())
            },
        }),
        json!({
            "name": "plaintext_config",
            "status": if key_state.plaintext_field_count == 0 { "ok" } else { "error" },
            "detail": if key_state.plaintext_field_count == 0 {
                "No plaintext auth_key values were found in config.".to_string()
            } else {
                format!(
                    "Plaintext auth_key values remain in config: {}. Run `krx-api-cli config seal`.",
                    key_state.plaintext_fields.join(", ")
                )
            },
        }),
        json!({
            "name": "selected_auth_key",
            "status": if auth_key_ready { "ok" } else { "error" },
            "detail": if key_state.seal_required {
                "Plaintext config values block API commands until they are sealed.".to_string()
            } else if auth_key_ready {
                format!("AUTH_KEY is ready for the selected environment via {auth_key_source}.")
            } else {
                format!("No AUTH_KEY is available for the selected environment. Expected env vars: {}, KRX_AUTH_KEY.", scoped_env_name(environment, "AUTH_KEY"))
            },
        }),
    ];

    let has_error = checks
        .iter()
        .any(|check| check.get("status").and_then(Value::as_str) == Some("error"));
    let summary = if key_state.seal_required {
        "Plaintext config secrets must be sealed before API commands will run."
    } else if auth_key_ready {
        "Local CLI configuration looks ready for the selected environment."
    } else {
        "AUTH_KEY is not ready for the selected environment."
    };

    print_json(
        &json!({
            "ok": !has_error,
            "version": env!("CARGO_PKG_VERSION"),
            "selected_environment": environment.as_str(),
            "catalog": {
                "category_count": manifest.category_count,
                "api_count": manifest.api_count,
            },
            "config": config_snapshot,
            "key_status": {
                "key_path": key_state.key_path,
                "key_exists": key_state.key_exists,
                "key_format": key_state.key_format,
                "previous_key_count": key_state.previous_key_count,
                "encrypted_field_count": key_state.encrypted_field_count,
                "plaintext_field_count": key_state.plaintext_field_count,
                "plaintext_fields": key_state.plaintext_fields,
                "seal_required": key_state.seal_required,
                "suggested_commands": key_state.suggested_commands,
            },
            "env_overrides": {
                "sample_auth_key": env_var_is_set("KRX_SAMPLE_AUTH_KEY"),
                "real_auth_key": env_var_is_set("KRX_REAL_AUTH_KEY"),
                "global_auth_key": env_var_is_set("KRX_AUTH_KEY"),
                "sample_base_url": env_var_is_set("KRX_SAMPLE_BASE_URL"),
                "real_base_url": env_var_is_set("KRX_REAL_BASE_URL"),
                "global_base_url": env_var_is_set("KRX_BASE_URL"),
                "user_agent": env_var_is_set("KRX_USER_AGENT"),
            },
            "selected_profile": {
                "auth_key_source": auth_key_source,
                "auth_key_ready": auth_key_ready,
                "base_url_source": base_url_source,
                "user_agent_source": user_agent_source,
            },
            "checks": checks,
            "summary": {
                "status": if has_error { "error" } else { "ok" },
                "message": summary,
            }
        }),
        compact,
    )
}

fn build_client(config_path: Option<&Path>, env: Environment) -> anyhow::Result<KrxClient> {
    let profile = resolve_profile(config_path, env)?;
    KrxClient::new(profile)
}

fn build_manifest_request(
    entry: &ApiEntry,
    matches: &ArgMatches,
    format: OutputFormat,
    environment: Environment,
) -> anyhow::Result<ApiRequest> {
    let path = if environment == Environment::Sample {
        entry.sample_path.clone()
    } else {
        entry.api_path.clone()
    };

    let mut query = Vec::new();
    for param in &entry.params {
        let raw_value = matches
            .get_one::<String>(&param.name)
            .ok_or_else(|| anyhow!("missing required parameter `{}`", param.name))?;
        validate_param_value(param, raw_value)?;
        query.push((param.name.clone(), raw_value.clone()));
    }

    Ok(ApiRequest {
        path,
        query,
        format,
    })
}

fn response_transform_from_matches(matches: &ArgMatches) -> anyhow::Result<ResponseTransform> {
    let filters = matches
        .get_many::<String>("filter")
        .map(|values| values.map(|value| parse_filter_expr(value)).collect())
        .transpose()?
        .unwrap_or_default();
    let sort_by = matches.get_one::<String>("sort_by").cloned();
    let sort_order = match matches.get_one::<String>("order").map(String::as_str) {
        Some("asc") | None => SortOrder::Asc,
        Some("desc") => SortOrder::Desc,
        Some(other) => return Err(anyhow!("unsupported sort order `{other}`")),
    };
    let limit = matches.get_one::<usize>("limit").copied();
    let select = matches.get_one::<String>("select").map(|value| {
        value
            .split(',')
            .map(str::trim)
            .filter(|field| !field.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    });

    if matches.contains_id("select") && select.as_ref().is_some_and(Vec::is_empty) {
        return Err(anyhow!(
            "select field list cannot be empty; use comma-separated response fields"
        ));
    }

    if limit == Some(0) {
        return Err(anyhow!("limit must be greater than zero"));
    }

    Ok(ResponseTransform {
        filters,
        sort_by,
        sort_order,
        limit,
        select,
    })
}

fn validate_param_value(param: &ApiParam, value: &str) -> anyhow::Result<()> {
    if param.name == "basDd"
        && (value.len() != 8 || !value.chars().all(|character| character.is_ascii_digit()))
    {
        return Err(anyhow!("`basDd` must be YYYYMMDD"));
    }

    Ok(())
}

fn parse_filter_expr(expression: &str) -> anyhow::Result<FilterExpr> {
    let mut parts = expression.splitn(3, ':');
    let field = parts.next().unwrap_or_default().trim();
    let operator = parts.next().unwrap_or_default().trim();
    let value = parts.next().unwrap_or_default().trim();

    if field.is_empty() || operator.is_empty() || value.is_empty() {
        return Err(anyhow!(
            "invalid --filter expression `{expression}`; expected FIELD:OP:VALUE with OP in eq, ne, gt, gte, lt, lte, contains"
        ));
    }

    let operator = match canonical_field_name(operator).as_str() {
        "eq" => FilterOperator::Eq,
        "ne" => FilterOperator::Ne,
        "gt" => FilterOperator::Gt,
        "gte" => FilterOperator::Gte,
        "lt" => FilterOperator::Lt,
        "lte" => FilterOperator::Lte,
        "contains" => FilterOperator::Contains,
        _ => {
            return Err(anyhow!(
            "invalid --filter operator `{operator}`; use one of eq, ne, gt, gte, lt, lte, contains"
        ))
        }
    };

    Ok(FilterExpr {
        field: field.to_string(),
        operator,
        value: value.to_string(),
    })
}

fn apply_response_transform(
    payload: ApiResponse,
    transform: &ResponseTransform,
) -> anyhow::Result<ApiResponse> {
    if !transform.is_active() {
        return Ok(payload);
    }

    match payload {
        ApiResponse::Json(value) => Ok(ApiResponse::Json(apply_json_transform(value, transform)?)),
        ApiResponse::Xml(_) => Err(anyhow!(
            "response transforms require JSON output; retry without `--format xml`"
        )),
    }
}

fn apply_json_transform(value: Value, transform: &ResponseTransform) -> anyhow::Result<Value> {
    let mut object = match value {
        Value::Object(object) => object,
        _ => {
            return Err(anyhow!(
                "response transforms require a JSON object with an array block like `OutBlock_1`"
            ));
        }
    };

    let target_key = locate_transform_target(&object)?;
    let rows = object
        .get_mut(&target_key)
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("response field `{target_key}` is not an array"))?;

    if !transform.filters.is_empty() {
        filter_rows(rows, &transform.filters)?;
    }

    if let Some(sort_by) = &transform.sort_by {
        sort_rows(rows, sort_by, transform.sort_order)?;
    }

    if let Some(limit) = transform.limit {
        rows.truncate(limit);
    }

    if let Some(select) = &transform.select {
        select_fields(rows, select)?;
    }

    Ok(Value::Object(object))
}

fn locate_transform_target(object: &Map<String, Value>) -> anyhow::Result<String> {
    if object
        .get("OutBlock_1")
        .and_then(Value::as_array)
        .is_some_and(|rows| !rows.is_empty())
    {
        return Ok("OutBlock_1".to_string());
    }

    object
        .iter()
        .find(|(_, value)| {
            value
                .as_array()
                .is_some_and(|rows| rows.iter().all(|row| row.is_object()))
        })
        .map(|(key, _)| key.clone())
        .ok_or_else(|| {
            anyhow!("response transforms require a JSON array block such as `OutBlock_1`")
        })
}

fn sort_rows(rows: &mut [Value], field: &str, sort_order: SortOrder) -> anyhow::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    let resolved = resolve_requested_field(rows, field, "--sort-by")?;

    rows.sort_by(|left, right| compare_row_values(left, right, &resolved.source_key, sort_order));
    Ok(())
}

fn filter_rows(rows: &mut Vec<Value>, filters: &[FilterExpr]) -> anyhow::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    let resolved_filters = filters
        .iter()
        .map(|filter| {
            Ok(ResolvedFilter {
                source_key: resolve_requested_field(rows, &filter.field, "--filter")?.source_key,
                operator: filter.operator,
                value: filter.value.clone(),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    rows.retain(|row| {
        resolved_filters
            .iter()
            .all(|filter| row_matches_filter(row, filter))
    });

    Ok(())
}

fn select_fields(rows: &mut [Value], fields: &[String]) -> anyhow::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    let resolved_fields = fields
        .iter()
        .map(|field| resolve_requested_field(rows, field, "--select"))
        .collect::<anyhow::Result<Vec<_>>>()?;

    for row in rows.iter_mut() {
        let object = row
            .as_object()
            .ok_or_else(|| anyhow!("response transform expected object rows"))?;
        let mut selected = Map::new();
        for field in &resolved_fields {
            if let Some(value) = object.get(&field.source_key) {
                selected.insert(field.output_key.clone(), value.clone());
            }
        }
        *row = Value::Object(selected);
    }

    Ok(())
}

fn resolve_requested_field(
    rows: &[Value],
    requested: &str,
    option_name: &str,
) -> anyhow::Result<ResolvedField> {
    if rows.iter().any(|row| row_field(row, requested).is_some()) {
        return Ok(ResolvedField {
            output_key: requested.to_string(),
            source_key: requested.to_string(),
        });
    }

    if let Some(actual_field) = available_fields(rows)
        .into_iter()
        .find(|field| field.eq_ignore_ascii_case(requested))
    {
        return Ok(ResolvedField {
            output_key: actual_field.clone(),
            source_key: actual_field,
        });
    }

    let normalized = canonical_field_name(requested);
    if let Some(alias) = FIELD_ALIASES.iter().find(|alias| alias.alias == normalized) {
        if let Some(source_key) = alias
            .candidates
            .iter()
            .find(|candidate| rows.iter().any(|row| row_field(row, candidate).is_some()))
        {
            return Ok(ResolvedField {
                output_key: alias.alias.to_string(),
                source_key: (*source_key).to_string(),
            });
        }

        return Err(anyhow!(
            "response field alias `{requested}` is not available for {option_name}; available fields: {}; supported aliases for this response: {}",
            available_fields(rows).join(", "),
            available_aliases(rows).join(", ")
        ));
    }

    Err(anyhow!(
        "unknown response field or alias `{requested}` for {option_name}; available fields: {}; supported aliases: {}",
        available_fields(rows).join(", "),
        supported_alias_names().join(", ")
    ))
}

fn compare_row_values(left: &Value, right: &Value, field: &str, sort_order: SortOrder) -> Ordering {
    match (row_field(left, field), row_field(right, field)) {
        (Some(left), Some(right)) => {
            let ordering = compare_scalar_values(left, right);
            match sort_order {
                SortOrder::Asc => ordering,
                SortOrder::Desc => ordering.reverse(),
            }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_scalar_values(left: &Value, right: &Value) -> Ordering {
    match (value_as_number(left), value_as_number(right)) {
        (Some(left), Some(right)) => left.partial_cmp(&right).unwrap_or(Ordering::Equal),
        _ => value_as_text(left).cmp(&value_as_text(right)),
    }
}

fn row_matches_filter(row: &Value, filter: &ResolvedFilter) -> bool {
    let Some(value) = row_field(row, &filter.source_key) else {
        return false;
    };

    match filter.operator {
        FilterOperator::Eq => compare_value_to_literal(value, &filter.value) == Ordering::Equal,
        FilterOperator::Ne => compare_value_to_literal(value, &filter.value) != Ordering::Equal,
        FilterOperator::Gt => compare_value_to_literal(value, &filter.value) == Ordering::Greater,
        FilterOperator::Gte => {
            let ordering = compare_value_to_literal(value, &filter.value);
            ordering == Ordering::Greater || ordering == Ordering::Equal
        }
        FilterOperator::Lt => compare_value_to_literal(value, &filter.value) == Ordering::Less,
        FilterOperator::Lte => {
            let ordering = compare_value_to_literal(value, &filter.value);
            ordering == Ordering::Less || ordering == Ordering::Equal
        }
        FilterOperator::Contains => {
            normalized_text(&value_as_text(value)).contains(&normalized_text(&filter.value))
        }
    }
}

fn compare_value_to_literal(value: &Value, literal: &str) -> Ordering {
    match (value_as_number(value), literal_as_number(literal)) {
        (Some(left), Some(right)) => left.partial_cmp(&right).unwrap_or(Ordering::Equal),
        _ => normalized_text(&value_as_text(value)).cmp(&normalized_text(literal)),
    }
}

fn value_as_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => {
            let normalized = text.trim().replace(',', "");
            if normalized.is_empty() {
                None
            } else {
                normalized.parse::<f64>().ok()
            }
        }
        _ => None,
    }
}

fn literal_as_number(value: &str) -> Option<f64> {
    let normalized = value.trim().replace(',', "");
    if normalized.is_empty() {
        None
    } else {
        normalized.parse::<f64>().ok()
    }
}

fn value_as_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

fn row_field<'a>(row: &'a Value, field: &str) -> Option<&'a Value> {
    row.as_object().and_then(|object| object.get(field))
}

fn available_fields(rows: &[Value]) -> Vec<String> {
    let mut fields = rows
        .iter()
        .find_map(|row| row.as_object())
        .map(|object| object.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    fields.sort();
    fields
}

fn available_aliases(rows: &[Value]) -> Vec<&'static str> {
    FIELD_ALIASES
        .iter()
        .filter(|alias| {
            alias
                .candidates
                .iter()
                .any(|candidate| rows.iter().any(|row| row_field(row, candidate).is_some()))
        })
        .map(|alias| alias.alias)
        .collect()
}

fn supported_alias_names() -> Vec<&'static str> {
    FIELD_ALIASES.iter().map(|alias| alias.alias).collect()
}

fn canonical_field_name(field: &str) -> String {
    field.trim().to_ascii_lowercase().replace('-', "_")
}

fn normalized_text(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn read_stdin_secret() -> anyhow::Result<String> {
    let mut buffer = String::new();
    io::stdin()
        .read_to_string(&mut buffer)
        .context("failed to read secret from stdin")?;

    let trimmed = buffer.trim().to_string();
    if trimmed.is_empty() {
        return Err(anyhow!("stdin secret was empty"));
    }

    Ok(trimmed)
}

fn output_format_from_matches(matches: &ArgMatches) -> anyhow::Result<OutputFormat> {
    match matches.get_one::<String>("format").map(String::as_str) {
        Some("json") | None => Ok(OutputFormat::Json),
        Some("xml") => Ok(OutputFormat::Xml),
        Some(other) => Err(anyhow!("unsupported format `{other}`")),
    }
}

fn environment_from_matches(matches: &ArgMatches) -> anyhow::Result<Environment> {
    environment_from_str(
        matches
            .get_one::<String>("env")
            .map(String::as_str)
            .unwrap_or("sample"),
    )
}

fn environment_from_str(value: &str) -> anyhow::Result<Environment> {
    match value {
        "sample" => Ok(Environment::Sample),
        "real" => Ok(Environment::Real),
        _ => Err(anyhow!("unsupported environment `{value}`")),
    }
}

fn env_var_is_set(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
}

fn scoped_env_name(environment: Environment, name: &str) -> String {
    format!("KRX_{}_{}", environment.as_str().to_uppercase(), name)
}

fn print_response(payload: &ApiResponse, compact: bool) -> anyhow::Result<()> {
    match payload {
        ApiResponse::Json(value) => print_json(value, compact),
        ApiResponse::Xml(value) => {
            println!("{value}");
            Ok(())
        }
    }
}

fn print_json<T: Serialize>(payload: &T, compact: bool) -> anyhow::Result<()> {
    let rendered = if compact {
        serde_json::to_string(payload)
    } else {
        serde_json::to_string_pretty(payload)
    }
    .context("failed to serialize JSON output")?;

    println!("{rendered}");
    Ok(())
}

fn api_transform_after_help() -> String {
    format!(
        "Client-side transforms:\n  --filter <FIELD:OP:VALUE>  Filter rows; repeatable. Example: change_rate:gte:10, name:contains:전자\n  --sort-by <FIELD>          Sort JSON list rows by a response field\n  --order <asc|desc>         Sort order for --sort-by\n  --limit <N>                Keep only the first N rows after sorting\n  --select <A,B,...>         Keep only the listed response fields in each row\n\nPreferred aliases:\n  {}",
        supported_alias_names().join(", ")
    )
}

fn top_level_after_help(manifest: &ApiManifest) -> String {
    let mut lines = Vec::new();
    lines.push("Top-level groups:".to_string());
    lines.push("  doctor   Local readiness and config diagnostics".to_string());
    lines.push("  config   Local config management".to_string());
    lines.push("  catalog  Embedded manifest summary/export".to_string());
    for category in &manifest.categories {
        lines.push(format!(
            "  {:<12} {} ({} APIs)",
            category.id, category.description, category.api_count
        ));
    }
    lines.join("\n")
}

fn category_long_about(category: &crate::manifest::Category) -> String {
    format!(
        "{}\n\nLabel: {}\nAPI count: {}",
        category.description, category.label, category.api_count
    )
}

fn api_long_about(entry: &ApiEntry) -> String {
    format!(
        "{}\n\nPath: {}\nSample path: {}\nSource: {}",
        entry.description, entry.api_path, entry.sample_path, entry.source_file
    )
}

fn leak_string(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

impl ResponseTransform {
    fn is_active(&self) -> bool {
        !self.filters.is_empty()
            || self.sort_by.is_some()
            || self.limit.is_some()
            || self.select.is_some()
    }
}
