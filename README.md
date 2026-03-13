# krx-api-cli

한국거래소 KRX Open API를 단일 네이티브 바이너리로 호출하기 위한 Rust CLI입니다.
공식 서비스 목록과 로컬 `docx` 명세를 기준으로 API 카탈로그를 내장하고,
카테고리와 API를 CLI 명령 트리로 그대로 노출합니다.

## 고지사항

- 이 프로젝트는 한국거래소의 공식 지원 도구가 아닙니다.
- 이 프로젝트의 사용으로 발생하는 결과와 책임은 사용자에게 있습니다.
- KRX Open API 정책, 승인 상태, 응답 형식 변경은 공식 문서와 공식 지원 채널을 우선 확인해야 합니다.

## 문서 구성

- 사람용 개요와 빠른 시작: `README.md`
- LLM/에이전트용 사용 규칙: `docs/LLM_GUIDE.md`
- 전체 명령 레퍼런스: `docs/CLI_REFERENCE.md`
- 기계가 읽는 원본 manifest: `data/krx_api_manifest.json`
- 저장소 작업 규칙: `AGENTS.md`
- 현재 구현 상태와 검증 기록: `STATE.md`

## 현재 범위

- 단일 Rust 바이너리
- `sample` / `real` 환경 전환
- OS별 외부 config 경로 사용
- KRX 서비스 목록 기반 embedded manifest
- 카테고리별/기능별 CLI help
- manifest 기반 REST 실행
- JSON/XML 응답 포맷 선택

현재는 OPPINFO004 서비스 목록에 노출된 31개 REST API를 대상으로 합니다.

## 빠른 시작

### 1. 바이너리 받기

```bash
curl -fsSL https://raw.githubusercontent.com/johwanghee/krx-api-cli/main/install.sh | bash
```

설치 스크립트는 다음을 자동으로 처리합니다.

- 현재 OS와 아키텍처 감지
- 최신 GitHub Release 확인
- 대응되는 release asset 다운로드
- 가능하면 `sha256sums.txt`로 checksum 검증
- 이미 설치되어 있으면 버전 비교 후 자동 업데이트 또는 no-op
- 기본 설치 경로 `~/.local/bin`

이 방식은 GitHub Release가 실제로 발행되어 있어야 동작합니다.
Release가 아직 없으면 아래 수동 설치나 소스 빌드를 사용하면 됩니다.

업데이트 정책:

- 스크립트를 다시 실행하면 설치 또는 업데이트를 자동으로 수행
- 이미 같은 버전이면 다운로드 없이 종료
- 더 낮은 버전을 설치하려면 `--allow-downgrade` 또는 `--force`
- 설치 계획만 보고 싶으면 `--check`

버전 고정이나 경로 변경도 가능합니다.

```bash
curl -fsSL https://raw.githubusercontent.com/johwanghee/krx-api-cli/main/install.sh | \
  bash -s -- --version v1.0.1 --install-dir ~/.local/bin
```

설치 계획 확인:

```bash
curl -fsSL https://raw.githubusercontent.com/johwanghee/krx-api-cli/main/install.sh | \
  bash -s -- --check
```

수동 설치가 필요하면 GitHub Releases 또는 GitHub Actions artifacts에서 OS별 prebuilt 바이너리를
직접 받아도 됩니다.

제공 대상:

- macOS x86_64: `krx-api-cli-macos-x86_64.tar.gz`
- macOS arm64: `krx-api-cli-macos-arm64.tar.gz`
- Linux x86_64: `krx-api-cli-linux-x86_64.tar.gz`
- Windows x86_64: `krx-api-cli-windows-x86_64.zip`

압축을 푼 뒤 실행 파일을 `PATH`에 두면 아래 예제를 그대로 사용할 수 있습니다.
현재 디렉터리에서 바로 실행할 때만 OS에 따라 다음처럼 앞에 경로를 붙이면 됩니다.

- macOS/Linux: `./krx-api-cli`
- Windows: `.\krx-api-cli.exe`

### 2. 설정 파일 경로 확인 및 초기화

```bash
krx-api-cli config path
krx-api-cli config init
krx-api-cli config key status
```

config와 암호화 key 파일은 저장소 밖 OS 전용 경로에 생성됩니다.

### 3. 인증키 설정

기본 원칙은 이렇습니다.

- `sample` 환경도 `real` 환경과 마찬가지로 `AUTH_KEY`를 명시해야 합니다.
- `real` 환경은 KRX에서 승인된 실제 `AUTH_KEY`가 필요합니다.
- `config set-auth-key`로 저장한 키는 로컬 key 파일을 이용해 암호화 저장됩니다.
- 환경변수 override는 계속 plaintext로 사용되며 별도 암호화하지 않습니다.
- 공개 샘플 키를 코드나 저장소에 내장하지 않습니다.
- 기존 config에 plaintext `auth_key`가 이미 있으면 API 호출 전에 차단되며 `config seal`로 마이그레이션해야 합니다.

예시:

```bash
krx-api-cli config set-auth-key --profile sample --stdin
krx-api-cli config set-auth-key --profile real --stdin
krx-api-cli config key status
krx-api-cli config seal
```

환경변수 override:

- `KRX_ENV`
- `KRX_CONFIG`
- `KRX_AUTH_KEY`
- `KRX_SAMPLE_AUTH_KEY`
- `KRX_REAL_AUTH_KEY`
- `KRX_BASE_URL`
- `KRX_SAMPLE_BASE_URL`
- `KRX_REAL_BASE_URL`
- `KRX_USER_AGENT`

`config show`는 민감값 원문 대신 저장 상태를 보여줍니다.

- `storage = "encrypted"`: config에 암호화 저장됨
- `storage = "plaintext"`: 구버전/수동 편집 상태, `config seal` 필요
- `storage = "absent"`: 값 없음

### 4. 대표 명령 실행

샘플 환경에서 KRX 시리즈 일별시세정보:

```bash
krx-api-cli index krx-dd-trd --bas-dd 20200414
```

실환경에서 같은 API 호출:

```bash
krx-api-cli --env real index krx-dd-trd --bas-dd 20240102
```

xml 응답:

```bash
krx-api-cli --format xml index krx-dd-trd --bas-dd 20200414
```

정렬/후처리 예시:

```bash
krx-api-cli --env real stock stk-bydd-trd --bas-dd 20260312 \
  --sort-by market_cap --order desc --limit 10 --select name,symbol,market_cap
```

필터 예시:

```bash
krx-api-cli --env real stock stk-bydd-trd --bas-dd 20260312 \
  --filter change_rate:gte:20 --sort-by change_rate --order desc \
  --select name,symbol,change_rate,market_cap
```

## 오류 출력

실패 시 stdout 대신 stderr로 구조화된 JSON envelope를 출력합니다.
로컬 입력 오류는 가능한 경우 인증키 확인이나 실제 API 호출보다 먼저 반환합니다.

- `program_error`
  - CLI 인자 오류, config 문제, 네트워크 실패, 내부 처리 실패
- `program_error.category=plaintext_secret_detected`
  - config에 평문 `auth_key`가 남아 있어서 의도적으로 실행을 막은 경우
- `api_error`
  - KRX 응답 자체의 실패
  - HTTP status 실패뿐 아니라 HTTP 200 + KRX 오류 코드/메시지도 포함

주요 필드:

- `error_type`
- `llm_hint.summary`
- `llm_hint.retryable`
- `llm_hint.next_action`
- `api_error.category`
- `program_error.category`
- `program_error.suggested_commands`
- `program_error.suggested_env_vars`

종료 코드:

- `2`: `program_error`
- `3`: `api_error`

## 응답 후처리

list 형태의 JSON 응답에는 공통 후처리 옵션을 붙일 수 있습니다.

- `--sort-by <FIELD>`
- `--order <asc|desc>`
- `--limit <N>`
- `--select <A,B,...>`
- `--filter <FIELD:OP:VALUE>`

이 기능은 KRX 서버 정렬이 아니라 CLI의 client-side 후처리입니다.
현재는 JSON array block, 보통 `OutBlock_1`에 적용됩니다.

지원 연산자:

- `eq`
- `ne`
- `gt`
- `gte`
- `lt`
- `lte`
- `contains`

권장 alias:

- `date`
- `name`
- `symbol`
- `market`
- `market_cap`
- `close_price`
- `open_price`
- `high_price`
- `low_price`
- `change_price`
- `change_rate`
- `volume`
- `value`
- `listed_shares`

## 명령 탐색

최상위 카테고리:

```bash
krx-api-cli --help
```

특정 카테고리:

```bash
krx-api-cli stock --help
```

특정 API:

```bash
krx-api-cli stock stk-bydd-trd --help
```

내장 카탈로그 요약/내보내기:

```bash
krx-api-cli catalog summary
krx-api-cli catalog export --compact
```

## Prebuilt 빌드

GitHub Actions는 다음 prebuilt 산출물을 만듭니다.

- `macos-15-intel`에서 빌드한 `krx-api-cli-macos-x86_64.tar.gz`
- `macos-15`에서 빌드한 `krx-api-cli-macos-arm64.tar.gz`
- `ubuntu-22.04`에서 빌드한 `krx-api-cli-linux-x86_64.tar.gz`
- `windows-2022`에서 빌드한 `krx-api-cli-windows-x86_64.zip`

동작 방식:

- `push`, `pull_request`, `workflow_dispatch` 때마다 전체 prebuilt 빌드를 수행합니다.
- 각 빌드 산출물은 GitHub Actions artifact로 업로드됩니다.
- `v*` 형식 태그를 push하면 같은 산출물과 `sha256sums.txt`를 GitHub Release 자산으로 업로드합니다.

## 소스에서 직접 빌드하기

prebuilt 바이너리 대신 로컬에서 직접 빌드하려면 아래를 사용합니다.

```bash
cargo build --release
```

빌드 결과:

- macOS/Linux: `./target/release/krx-api-cli`
- Windows: `.\target\release\krx-api-cli.exe`

## 재생성

manifest 재생성:

```bash
python3 tools/sync_krx_specs.py
python3 tools/generate_manifest.py
```

`tools/generate_manifest.py`는 `krx_docs/service_catalog.json`이 있으면 이를 우선 읽고,
포털 목록과 `krx_docs/specs` 사이에 누락/여분이 있으면 stderr로 경고합니다.

누락분만 보충:

```bash
python3 tools/sync_krx_specs.py --missing-only
```

CLI reference 재생성:

```bash
python3 tools/render_cli_reference.py
```
