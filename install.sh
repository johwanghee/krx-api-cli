#!/usr/bin/env bash

set -euo pipefail

REPO_OWNER="johwanghee"
REPO_NAME="krx-api-cli"
REPO_SLUG="${REPO_OWNER}/${REPO_NAME}"
API_BASE_URL="${KRX_API_CLI_API_BASE_URL:-https://api.github.com/repos/${REPO_SLUG}}"
RELEASE_BASE_URL="${KRX_API_CLI_RELEASE_BASE_URL:-https://github.com/${REPO_SLUG}/releases/download}"
DEFAULT_INSTALL_DIR="${HOME}/.local/bin"
DEFAULT_BINARY_NAME="krx-api-cli"

VERSION="${KRX_API_CLI_VERSION:-latest}"
INSTALL_DIR="${KRX_API_CLI_INSTALL_DIR:-${DEFAULT_INSTALL_DIR}}"
DRY_RUN=0
CHECK_ONLY=0
FORCE=0
ALLOW_DOWNGRADE=0
INSTALLED_PATH=""
INSTALLED_VERSION=""
INSTALL_ACTION=""

usage() {
  cat <<'EOF'
Install krx-api-cli from GitHub Releases.

Usage:
  curl -fsSL https://raw.githubusercontent.com/johwanghee/krx-api-cli/main/install.sh | bash
  curl -fsSL https://raw.githubusercontent.com/johwanghee/krx-api-cli/main/install.sh | bash -s -- --version v1.0.2

Options:
  --version <tag|latest>   Release tag to install. Default: latest
  --install-dir <path>     Destination directory. Default: ~/.local/bin
  --check                  Print a JSON install/update plan and exit
  --dry-run                Resolve the release asset and print the plan without installing
  --force                  Reinstall even when the target version is already installed
  --allow-downgrade        Allow installing a version lower than the currently installed version
  -h, --help               Show this help

Environment:
  KRX_API_CLI_VERSION           Same as --version
  KRX_API_CLI_INSTALL_DIR       Same as --install-dir
  KRX_API_CLI_API_BASE_URL      Override the release metadata API base URL
  KRX_API_CLI_RELEASE_BASE_URL  Override the release asset base URL
EOF
}

log() {
  printf '%s\n' "$*" >&2
}

fail() {
  log "error: $*"
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"
}

json_string_field() {
  local key="$1"
  sed -n "s/^[[:space:]]*\"${key}\":[[:space:]]*\"\\([^\"]*\\)\".*/\\1/p" | head -n 1
}

json_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

normalize_version() {
  case "$1" in
    latest) printf '%s\n' "latest" ;;
    v*) printf '%s\n' "$1" ;;
    *) printf 'v%s\n' "$1" ;;
  esac
}

compare_versions() {
  local left="${1#v}"
  local right="${2#v}"

  awk -v left="${left}" -v right="${right}" '
    function part(value, idx, pieces) {
      split(value, pieces, ".")
      return (idx in pieces) ? pieces[idx] + 0 : 0
    }
    BEGIN {
      for (i = 1; i <= 6; i++) {
        l = part(left, i)
        r = part(right, i)
        if (l < r) {
          print -1
          exit
        }
        if (l > r) {
          print 1
          exit
        }
      }
      print 0
    }
  '
}

resolve_release_tag() {
  if [ "${VERSION}" = "latest" ]; then
    local response
    response="$(curl -fsSL -H 'Accept: application/vnd.github+json' "${API_BASE_URL}/releases/latest")" \
      || fail "failed to resolve the latest GitHub release (no GitHub Release may be published yet)"
    VERSION="$(printf '%s\n' "${response}" | json_string_field "tag_name")"
    [ -n "${VERSION}" ] || fail "latest GitHub release did not include a tag_name"
    return
  fi

  VERSION="$(normalize_version "${VERSION}")"
  curl -fsSL -H 'Accept: application/vnd.github+json' \
    "${API_BASE_URL}/releases/tags/${VERSION}" >/dev/null \
    || fail "GitHub release ${VERSION} does not exist (the tag may exist without a published Release)"
}

detect_platform() {
  local uname_s uname_m
  uname_s="$(uname -s)"
  uname_m="$(uname -m)"

  case "${uname_s}" in
    Linux)
      PLATFORM="linux"
      ARCHIVE_EXT="tar.gz"
      BINARY_NAME="${DEFAULT_BINARY_NAME}"
      ;;
    Darwin)
      PLATFORM="macos"
      ARCHIVE_EXT="tar.gz"
      BINARY_NAME="${DEFAULT_BINARY_NAME}"
      ;;
    MINGW*|MSYS*|CYGWIN*)
      PLATFORM="windows"
      ARCHIVE_EXT="zip"
      BINARY_NAME="${DEFAULT_BINARY_NAME}.exe"
      ;;
    *)
      fail "unsupported operating system: ${uname_s}"
      ;;
  esac

  case "${uname_m}" in
    x86_64|amd64)
      ARCH="x86_64"
      ;;
    arm64|aarch64)
      case "${PLATFORM}" in
        macos)
          ARCH="arm64"
          ;;
        *)
          fail "unsupported architecture: ${uname_m} on ${PLATFORM} (available release assets currently target macOS arm64 and x86_64, plus Linux/Windows x86_64)"
          ;;
      esac
      ;;
    *)
      fail "unsupported architecture: ${uname_m}"
      ;;
  esac

  ARCHIVE_NAME="${DEFAULT_BINARY_NAME}-${PLATFORM}-${ARCH}.${ARCHIVE_EXT}"
  ARCHIVE_URL="${RELEASE_BASE_URL}/${VERSION}/${ARCHIVE_NAME}"
  CHECKSUM_URL="${RELEASE_BASE_URL}/${VERSION}/sha256sums.txt"
}

check_release_asset() {
  curl -fsI -L "${ARCHIVE_URL}" >/dev/null \
    || fail "release asset not found for ${VERSION}: ${ARCHIVE_NAME}"
}

find_installed_binary() {
  if [ -x "${INSTALL_DIR}/${BINARY_NAME}" ]; then
    printf '%s\n' "${INSTALL_DIR}/${BINARY_NAME}"
    return 0
  fi

  if command -v "${BINARY_NAME}" >/dev/null 2>&1; then
    command -v "${BINARY_NAME}"
    return 0
  fi

  return 1
}

read_installed_version() {
  local binary_path="$1"
  local raw

  raw="$("${binary_path}" --version 2>/dev/null | awk 'NF { print $NF; exit }')" || return 1
  [ -n "${raw}" ] || return 1
  normalize_version "${raw}"
}

detect_installed_state() {
  INSTALLED_PATH=""
  INSTALLED_VERSION=""

  if ! INSTALLED_PATH="$(find_installed_binary)"; then
    return 0
  fi

  if INSTALLED_VERSION="$(read_installed_version "${INSTALLED_PATH}")"; then
    return 0
  fi

  INSTALLED_VERSION=""
}

plan_install_action() {
  if [ -z "${INSTALLED_PATH}" ]; then
    INSTALL_ACTION="install"
    return 0
  fi

  if [ -z "${INSTALLED_VERSION}" ]; then
    INSTALL_ACTION="reinstall"
    return 0
  fi

  local comparison
  comparison="$(compare_versions "${INSTALLED_VERSION}" "${VERSION}")"

  case "${comparison}" in
    -1)
      INSTALL_ACTION="update"
      ;;
    0)
      if [ "${FORCE}" -eq 1 ]; then
        INSTALL_ACTION="reinstall"
      else
        INSTALL_ACTION="noop"
      fi
      ;;
    1)
      if [ "${FORCE}" -eq 1 ] || [ "${ALLOW_DOWNGRADE}" -eq 1 ]; then
        INSTALL_ACTION="downgrade"
      else
        INSTALL_ACTION="downgrade_blocked"
      fi
      ;;
    *)
      fail "failed to compare versions: installed=${INSTALLED_VERSION}, target=${VERSION}"
      ;;
  esac
}

emit_check_report() {
  local installed_path_json="null"
  local installed_version_json="null"
  local message update_available needs_install force_json allow_downgrade_json

  if [ -n "${INSTALLED_PATH}" ]; then
    installed_path_json="\"$(json_escape "${INSTALLED_PATH}")\""
  fi

  if [ -n "${INSTALLED_VERSION}" ]; then
    installed_version_json="\"$(json_escape "${INSTALLED_VERSION}")\""
  fi

  case "${INSTALL_ACTION}" in
    install)
      message="Binary is not installed in the target location and will be installed."
      update_available=true
      needs_install=true
      ;;
    update)
      message="A newer release is available and will replace the installed binary."
      update_available=true
      needs_install=true
      ;;
    reinstall)
      message="The target release will be installed over the existing binary."
      update_available=false
      needs_install=true
      ;;
    downgrade)
      message="The requested release is older than the installed binary and will be installed because downgrade is allowed."
      update_available=false
      needs_install=true
      ;;
    downgrade_blocked)
      message="The requested release is older than the installed binary. Re-run with --allow-downgrade or --force to install it."
      update_available=false
      needs_install=false
      ;;
    noop)
      message="The installed binary already matches the requested release."
      update_available=false
      needs_install=false
      ;;
    *)
      message="Unknown install action."
      update_available=false
      needs_install=false
      ;;
  esac

  if [ "${FORCE}" -eq 1 ]; then
    force_json=true
  else
    force_json=false
  fi

  if [ "${ALLOW_DOWNGRADE}" -eq 1 ]; then
    allow_downgrade_json=true
  else
    allow_downgrade_json=false
  fi

  cat <<EOF
{"ok":true,"binary_name":"$(json_escape "${BINARY_NAME}")","installed_path":${installed_path_json},"installed_version":${installed_version_json},"target_version":"$(json_escape "${VERSION}")","action":"$(json_escape "${INSTALL_ACTION}")","update_available":${update_available},"needs_install":${needs_install},"force":${force_json},"allow_downgrade":${allow_downgrade_json},"install_dir":"$(json_escape "${INSTALL_DIR}")","platform":"$(json_escape "${PLATFORM}")","arch":"$(json_escape "${ARCH}")","archive":"$(json_escape "${ARCHIVE_NAME}")","url":"$(json_escape "${ARCHIVE_URL}")","message":"$(json_escape "${message}")"}
EOF
}

sha256_file() {
  local path="$1"

  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${path}" | awk '{print $1}'
    return
  fi

  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${path}" | awk '{print $1}'
    return
  fi

  if command -v openssl >/dev/null 2>&1; then
    openssl dgst -sha256 "${path}" | awk '{print $NF}'
    return
  fi

  fail "no SHA-256 tool found (expected one of: sha256sum, shasum, openssl)"
}

extract_archive() {
  local archive_path="$1"
  local destination="$2"

  case "${ARCHIVE_EXT}" in
    tar.gz)
      need_cmd tar
      tar -xzf "${archive_path}" -C "${destination}"
      ;;
    zip)
      if command -v unzip >/dev/null 2>&1; then
        unzip -q "${archive_path}" -d "${destination}"
        return
      fi
      if command -v bsdtar >/dev/null 2>&1; then
        bsdtar -xf "${archive_path}" -C "${destination}"
        return
      fi
      if command -v tar >/dev/null 2>&1; then
        tar -xf "${archive_path}" -C "${destination}"
        return
      fi
      fail "no zip extraction tool found (expected one of: unzip, bsdtar, tar)"
      ;;
    *)
      fail "unsupported archive extension: ${ARCHIVE_EXT}"
      ;;
  esac
}

verify_checksum_if_available() {
  local archive_path="$1"
  local checksum_path="$2"

  if ! curl -fsSL "${CHECKSUM_URL}" -o "${checksum_path}"; then
    log "warning: sha256sums.txt not available for ${VERSION}; skipping checksum verification"
    return
  fi

  local expected actual
  expected="$(awk -v name="${ARCHIVE_NAME}" '$2 == name { print $1 }' "${checksum_path}")"
  if [ -z "${expected}" ]; then
    log "warning: no checksum entry found for ${ARCHIVE_NAME}; skipping checksum verification"
    return
  fi

  actual="$(sha256_file "${archive_path}")"
  [ "${expected}" = "${actual}" ] \
    || fail "checksum mismatch for ${ARCHIVE_NAME}"
}

install_binary() {
  local source_path="$1"
  local destination_path="${INSTALL_DIR}/${BINARY_NAME}"

  mkdir -p "${INSTALL_DIR}"
  cp "${source_path}" "${destination_path}"
  chmod 755 "${destination_path}" || true

  log "installed ${BINARY_NAME} to ${destination_path}"
  case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) ;;
    *)
      log "warning: ${INSTALL_DIR} is not currently on PATH"
      ;;
  esac
}

parse_args() {
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --version)
        shift
        [ "$#" -gt 0 ] || fail "--version requires a value"
        VERSION="$1"
        ;;
      --install-dir)
        shift
        [ "$#" -gt 0 ] || fail "--install-dir requires a value"
        INSTALL_DIR="$1"
        ;;
      --check)
        CHECK_ONLY=1
        ;;
      --dry-run)
        DRY_RUN=1
        ;;
      --force)
        FORCE=1
        ;;
      --allow-downgrade)
        ALLOW_DOWNGRADE=1
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        fail "unknown argument: $1"
        ;;
    esac
    shift
  done
}

main() {
  parse_args "$@"
  need_cmd curl
  need_cmd uname
  need_cmd mktemp
  need_cmd awk
  need_cmd find

  resolve_release_tag
  detect_platform
  check_release_asset
  detect_installed_state
  plan_install_action

  if [ "${CHECK_ONLY}" -eq 1 ]; then
    emit_check_report
    exit 0
  fi

  if [ "${DRY_RUN}" -eq 1 ]; then
    printf 'version=%s\nplatform=%s\narch=%s\narchive=%s\nurl=%s\ninstall_dir=%s\naction=%s\ninstalled_path=%s\ninstalled_version=%s\n' \
      "${VERSION}" \
      "${PLATFORM}" \
      "${ARCH}" \
      "${ARCHIVE_NAME}" \
      "${ARCHIVE_URL}" \
      "${INSTALL_DIR}" \
      "${INSTALL_ACTION}" \
      "${INSTALLED_PATH:-}" \
      "${INSTALLED_VERSION:-}"
    exit 0
  fi

  case "${INSTALL_ACTION}" in
    noop)
      log "${BINARY_NAME} ${VERSION} is already installed at ${INSTALLED_PATH}"
      exit 0
      ;;
    downgrade_blocked)
      fail "installed version ${INSTALLED_VERSION} is newer than target ${VERSION}; re-run with --allow-downgrade or --force"
      ;;
    install)
      log "installing ${BINARY_NAME} ${VERSION}"
      ;;
    update)
      log "updating ${BINARY_NAME} from ${INSTALLED_VERSION} to ${VERSION}"
      ;;
    reinstall)
      if [ -n "${INSTALLED_VERSION}" ]; then
        log "reinstalling ${BINARY_NAME} ${INSTALLED_VERSION}"
      else
        log "installing over existing ${BINARY_NAME} with unknown version metadata"
      fi
      ;;
    downgrade)
      log "downgrading ${BINARY_NAME} from ${INSTALLED_VERSION} to ${VERSION}"
      ;;
    *)
      fail "unsupported install action: ${INSTALL_ACTION}"
      ;;
  esac

  local tmpdir archive_path checksum_path binary_source
  tmpdir="$(mktemp -d)"
  trap "rm -rf '${tmpdir}'" EXIT INT TERM

  archive_path="${tmpdir}/${ARCHIVE_NAME}"
  checksum_path="${tmpdir}/sha256sums.txt"

  log "downloading ${ARCHIVE_NAME} from ${VERSION}"
  curl -fsSL "${ARCHIVE_URL}" -o "${archive_path}" \
    || fail "failed to download ${ARCHIVE_NAME}"

  verify_checksum_if_available "${archive_path}" "${checksum_path}"
  extract_archive "${archive_path}" "${tmpdir}"

  binary_source="$(find "${tmpdir}" -type f -name "${BINARY_NAME}" | head -n 1)"
  [ -n "${binary_source}" ] || fail "failed to find ${BINARY_NAME} inside ${ARCHIVE_NAME}"

  install_binary "${binary_source}"
  log "run \`${BINARY_NAME} --help\` to confirm the installation"
}

main "$@"
