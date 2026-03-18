# STATE.md

## Snapshot

- Date: 2026-03-18
- Status: manifest-driven KRX CLI implemented with encrypted local AUTH_KEY storage
- Repository started as an almost-empty git repository with local `krx_docs` specs only.
- Local git metadata was reinitialized after removing a mistakenly embedded sample `AUTH_KEY` from the work tree design.

## Completed

- Confirmed the KRX Open API service list page and per-service sample pages.
- Confirmed current public portal behavior as of 2026-03-13:
  - service list page: `https://openapi.krx.co.kr/contents/OPP/INFO/service/OPPINFO004.cmd`
  - sample endpoint pattern: `/svc/sample/apis/<group>/<api_id>`
  - real endpoint pattern: `/svc/apis/<group>/<api_id>`
  - request auth header name: `AUTH_KEY`
- Parsed local `docx` specs under `krx_docs/specs` and generated an embedded manifest:
  - categories: `7`
  - APIs: `31`
- Added Rust crate with:
  - dynamic CLI command tree from embedded manifest
  - `config`, `catalog`, and category/API subcommands
  - `doctor` top-level diagnostics for local readiness, env override presence, and config encryption state
  - sample/real environment selection
  - json/xml response format selection
  - manifest-driven GET executor
  - client-side list transforms for JSON responses (`--filter`, `--sort-by`, `--order`, `--limit`, `--select`)
  - LLM-friendly response field aliases (`name`, `symbol`, `market_cap`, `change_rate`, etc.)
  - structured JSON error reporting for `api_error` and `program_error`
  - error classification tuned for LLM remediation (`missing_auth_key`, `invalid_input`, `config_error`, `network_*`, `auth_or_permission`, `rate_limited`, etc.)
  - local config encryption for `auth_key` using a per-user key file
  - `config key status` and `config seal`
  - plaintext config secret blocking with `program_error.category=plaintext_secret_detected`
- Added project documents:
  - `README.md`
  - `docs/LLM_GUIDE.md`
  - `docs/CLI_REFERENCE.md`
  - `SPEC.md`
  - `AGENTS.md`
  - `STATE.md`
- Added release/install surface aligned with `kis-trading-cli`:
  - `.github/workflows/prebuilt.yml`
  - `install.sh`
  - install/update/manual install sections in `README.md`
  - install guidance in `docs/LLM_GUIDE.md`
- Added generator/renderer tools:
  - `tools/sync_krx_specs.py`
  - `tools/generate_manifest.py`
  - `tools/render_cli_reference.py`
- Added `krx_docs/service_catalog.json` as sync metadata output for the official download flow.
- Updated `tools/generate_manifest.py` to prefer `krx_docs/service_catalog.json`, attach portal metadata to manifest entries, and warn on catalog/spec mismatches.
- Removed the embedded sample `AUTH_KEY` from:
  - runtime config fallback
  - config template
  - embedded manifest
  - repo docs
- Changed current behavior so both `sample` and `real` environments require an explicit `AUTH_KEY` through config or environment variables.

## Active Decisions

- Language/runtime: Rust native binary
- HTTP client: `reqwest` + `rustls`
- Config format: TOML
- Config storage: OS-specific app config directory
- Command surface source: generated manifest from local KRX `docx` specs
- Distribution surface:
  - GitHub Actions prebuilt artifacts for macOS/Linux/Windows
  - GitHub Releases assets on `v*` tags
  - `install.sh` for install/update/check planning
- Documentation strategy:
  - `README.md` for people
  - `docs/LLM_GUIDE.md` for LLM/agent execution rules
  - `docs/CLI_REFERENCE.md` as a generated full command reference
  - `data/krx_api_manifest.json` as machine-readable source
- Security strategy:
  - no sample `AUTH_KEY` embedded in source, generated data, or docs
  - both `sample` and `real` require explicit `AUTH_KEY`
  - config values stay outside the repository
  - config-stored `auth_key` values are encrypted at rest
  - env var overrides remain plaintext and are not encrypted by the CLI
  - plaintext `auth_key` values left in config are rejected until sealed
- Current visible command model:
  - `config`
  - `catalog`
  - `index`
  - `stock`
  - `etp`
  - `bond`
  - `derivatives`
  - `general`
  - `esg`

## Verification

- `python3 tools/generate_manifest.py`: passed
- `python3 tools/generate_manifest.py` with temporary extra docx: mismatch warning confirmed
- `python3 tools/render_cli_reference.py`: passed
- `python3 tools/sync_krx_specs.py`: passed, `service_count=31`, `updated=31`
- `python3 tools/sync_krx_specs.py --missing-only`: passed, `service_count=31`, `skipped_existing=31`
- `bash -n install.sh`: passed
- `bash install.sh --help`: passed
- `.github/workflows/prebuilt.yml`: file created and manually inspected; local YAML parser module was unavailable for additional parse validation
- `cargo fmt -- --check`: passed
- `cargo check`: passed
- `cargo run -- --help`: passed
- `cargo run -- --compact catalog summary`: passed
- `cargo run -- --compact index krx-dd-trd --bas-dd 20200414`: passed before sample key removal, using an explicit sample key available at runtime
- `cargo run -- --format xml index krx-dd-trd --bas-dd 20200414`: passed before sample key removal, using an explicit sample key available at runtime
- `cargo run -- --config <temp>/config.toml config init --force`: passed
- `cargo run -- --config <temp>/config.toml --compact config show`: passed
- `cargo run -- --config <temp>/config.toml --compact index krx-dd-trd --bas-dd 20200414`: now correctly fails with `program_error` when `AUTH_KEY` is missing
- `cargo run -- --compact index krx-dd-trd --bas-dd 2024`: `program_error.category=invalid_input` confirmed before auth lookup
- `KRX_SAMPLE_AUTH_KEY=BAD cargo run -- --compact index krx-dd-trd --bas-dd 20200414`: `api_error.category=auth_or_permission` confirmed
- `cargo run -- config set-auth-key`: clap failure rendered as `program_error.category=invalid_input` with `suggested_commands`
- `cargo test`: passed
- `cargo run -- --config <temp>/config.toml --compact config key status`: passed
- `cargo run -- --config <temp>/config.toml --compact config show`: now shows `encrypted/plaintext/absent` secret storage state
- `cargo run -- --config <temp>/config.toml --compact config seal`: passed
- `cargo run -- --config <temp>/config.toml --compact index krx-dd-trd --bas-dd 20200414` with plaintext config key: `program_error.category=plaintext_secret_detected`
- `KRX_SAMPLE_AUTH_KEY=BAD cargo run -- --config <temp>/config.toml --compact index krx-dd-trd --bas-dd 20200414`: `api_error.category=auth_or_permission` confirmed with plaintext env override
- `cargo run -- --compact config key status`: passed on the real local config after sealing
- `cargo run -- --compact config seal`: passed on the real local config, `encrypted_fields=2`
- `cargo run -- --compact doctor`: passed, returns `ok=true` with local readiness summary
- `cargo run -- --config <temp>/config.toml config set-auth-key --profile sample --stdin` on a TTY: now prompts with a hidden-input AUTH_KEY message and succeeds
- `printf '%s' 'PIPE_SECRET_123' | cargo run -- --config <temp>/config.toml --compact config set-auth-key --profile real --stdin`: passed
- `cargo run -- --env real --compact stock stk-bydd-trd --bas-dd 20260312 --limit 1 --sort-by market_cap --order desc --select name,symbol,market_cap`: passed after sealing the real local config
- `cargo run -- --compact --env real stock stk-bydd-trd --bas-dd 20260312 --sort-by MKTCAP --order desc --limit 10 --select ISU_NM,ISU_CD,MKTCAP`: passed
- `cargo run -- --compact --format xml --env real stock stk-bydd-trd --bas-dd 20260312 --limit 10`: fails as `program_error.category=invalid_input`
- `cargo run -- --compact --env real stock stk-bydd-trd --bas-dd 20260312 --sort-by market_cap --order desc --limit 3 --select name,symbol,market_cap`: passed
- `cargo run -- --compact --env real stock stk-bydd-trd --bas-dd 20260312 --filter change_rate:gte:20 --sort-by change_rate --order desc --limit 3 --select name,symbol,change_rate`: passed
- `cargo run -- --compact --env real stock stk-bydd-trd --bas-dd 20260312 --filter name:contains:전자 --select name,symbol,market_cap`: passed

## Risks / Blockers

- Sample and real access both depend on a valid `AUTH_KEY`; no credentials are stored in the repo for smoke tests.
- Losing the local config key file prevents decryption of config-stored `auth_key` values until they are re-entered.
- `install.sh --check` and real install/update flow need a published GitHub Release to be exercised end-to-end.
- `docx` parsing assumes the current KRX spec document structure stays stable.
- The current executor is GET-only because the current local spec set is query-oriented.
- There is no automated test suite yet; current verification is smoke-test based.
- The sync tool depends on the current HTML structure of the KRX service list and detail pages.

## Next

- Add more unit tests around manifest loading and API response parsing.
- Tighten manifest generation so future spec shape changes fail loudly.
- Add richer validation for request parameters beyond `basDd`.
