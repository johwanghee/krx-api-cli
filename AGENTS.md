# AGENTS.md

## Goal
- Build a cross-platform Rust CLI for KRX Open API.
- The binary should expose the official KRX service catalog as stable CLI commands on macOS, Linux, and Windows.

## Source Of Truth
- Official service list: https://openapi.krx.co.kr/contents/OPP/INFO/service/OPPINFO004.cmd
- Local API specs: `krx_docs/specs/*.docx`
- Prefer the official portal and downloaded KRX specs over third-party wrappers or blogs.
- When behavior differs between portal HTML, sample pages, and `docx` specs, record the discrepancy in `STATE.md` before changing code.

## Current MVP Boundary
- `config init`: write a local config template outside the repo.
- `config set-auth-key`: store `AUTH_KEY` per environment in local config.
- `install.sh`: install/update the binary from GitHub Releases.
- `.github/workflows/prebuilt.yml`: build and publish prebuilt release assets.
- Embed the KRX API catalog in the binary from local `docx` specs.
- Expose API categories and functions as CLI commands so `--help` reveals the command surface.
- Use a manifest-driven executor for REST APIs instead of raw URL-first UX.
- Support `sample` and `real` environments and `json`/`xml` output.

## Engineering Rules
- Do not commit real `AUTH_KEY` values or any credential-like value.
- Do not embed public sample `AUTH_KEY` values in code, generated data, or docs.
- Keep output JSON-first so the CLI composes well with shell tooling and agents.
- Keep config in OS-specific app directories, not in the repository.
- Favor `reqwest` with `rustls` to avoid platform-specific OpenSSL packaging issues.
- Update `SPEC.md` when scope changes and `STATE.md` after meaningful implementation progress.
- If generated files change, regenerate them from the tool chain instead of hand-editing the generated output when practical.

## Generated Artifacts
- Spec sync tool: `tools/sync_krx_specs.py`
- Downloaded service metadata: `krx_docs/service_catalog.json`
- Manifest generator: `tools/generate_manifest.py`
- Embedded manifest output: `data/krx_api_manifest.json`
- CLI reference renderer: `tools/render_cli_reference.py`
- Generated CLI reference: `docs/CLI_REFERENCE.md`
- Release installer: `install.sh`
- Prebuilt CI workflow: `.github/workflows/prebuilt.yml`

## Workflow Preference
- Keep the repository self-describing for later context recovery.
- After a meaningful work unit, ensure `AGENTS.md`, `STATE.md`, `README.md`, and generated docs still reflect reality.
- Do not assume prior conversation context will be available on the next turn.

## Verification
- Preferred checks: `cargo fmt`, `cargo check`, `cargo run -- --help`
- Installer checks: `bash -n install.sh`, `bash install.sh --help`
- When API behavior changes, also verify one representative sample request and one config flow.
- If verification cannot run, note the exact blocker in `STATE.md` and in the final response.
