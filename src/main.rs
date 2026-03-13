mod api;
mod cli;
mod config;
mod errors;
mod manifest;

use std::path::Path;
use std::{io, io::Read};

use anyhow::{anyhow, Context};
use clap::error::ErrorKind;
use clap::{Arg, ArgAction, ArgMatches, Command};
use serde::Serialize;
use serde_json::json;

use crate::api::{ApiRequest, ApiResponse, KrxClient, OutputFormat};
use crate::cli::Environment;
use crate::config::{
    app_paths, redacted_config_value, resolve_profile, set_auth_key, write_config_template,
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
            let client = build_client(config_path.map(Path::new), env)?;
            let payload = client.send_request(request)?;
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
                .long_about(api_long_about(entry));

            for param in &entry.params {
                api_command = api_command.arg(api_arg(param));
            }

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
        .subcommand(Command::new("path").about("Show config path"))
        .subcommand(Command::new("show").about("Show redacted config"))
        .subcommand(
            Command::new("set-auth-key")
                .about("Store AUTH_KEY in config")
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
                }),
                compact,
            )
        }
        Some(("show", _)) => {
            let value = redacted_config_value(config_path.map(Path::new))?;
            print_json(&value, compact)
        }
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

            let written_path = set_auth_key(config_path.map(Path::new), environment, &value)?;
            print_json(
                &json!({
                    "ok": true,
                    "profile": environment,
                    "config_path": written_path,
                }),
                compact,
            )
        }
        _ => Err(anyhow!("unknown config subcommand")),
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

fn validate_param_value(param: &ApiParam, value: &str) -> anyhow::Result<()> {
    if param.name == "basDd"
        && (value.len() != 8 || !value.chars().all(|character| character.is_ascii_digit()))
    {
        return Err(anyhow!("`basDd` must be YYYYMMDD"));
    }

    Ok(())
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

fn top_level_after_help(manifest: &ApiManifest) -> String {
    let mut lines = Vec::new();
    lines.push("Top-level groups:".to_string());
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
