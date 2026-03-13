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

### 1. 빌드

```bash
cargo build --release
```

### 2. 설정 파일 경로 확인 및 초기화

```bash
./target/release/krx-api-cli config path
./target/release/krx-api-cli config init
```

config는 저장소 밖 OS 전용 경로에 생성됩니다.

### 3. 인증키 설정

기본 원칙은 이렇습니다.

- `sample` 환경도 `real` 환경과 마찬가지로 `AUTH_KEY`를 명시해야 합니다.
- `real` 환경은 KRX에서 승인된 실제 `AUTH_KEY`가 필요합니다.
- 실제 키는 `config set-auth-key` 또는 환경변수로 넣습니다.
- 공개 샘플 키를 코드나 저장소에 내장하지 않습니다.

예시:

```bash
./target/release/krx-api-cli config set-auth-key --profile sample --stdin
./target/release/krx-api-cli config set-auth-key --profile real --stdin
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

### 4. 대표 명령 실행

샘플 환경에서 KRX 시리즈 일별시세정보:

```bash
./target/release/krx-api-cli index krx-dd-trd --bas-dd 20200414
```

실환경에서 같은 API 호출:

```bash
./target/release/krx-api-cli --env real index krx-dd-trd --bas-dd 20240102
```

xml 응답:

```bash
./target/release/krx-api-cli --format xml index krx-dd-trd --bas-dd 20200414
```

정렬/후처리 예시:

```bash
./target/release/krx-api-cli --env real stock stk-bydd-trd --bas-dd 20260312 \
  --sort-by market_cap --order desc --limit 10 --select name,symbol,market_cap
```

필터 예시:

```bash
./target/release/krx-api-cli --env real stock stk-bydd-trd --bas-dd 20260312 \
  --filter change_rate:gte:20 --sort-by change_rate --order desc \
  --select name,symbol,change_rate,market_cap
```

## 오류 출력

실패 시 stdout 대신 stderr로 구조화된 JSON envelope를 출력합니다.
로컬 입력 오류는 가능한 경우 인증키 확인이나 실제 API 호출보다 먼저 반환합니다.

- `program_error`
  - CLI 인자 오류, config 문제, 네트워크 실패, 내부 처리 실패
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
./target/release/krx-api-cli --help
```

특정 카테고리:

```bash
./target/release/krx-api-cli stock --help
```

특정 API:

```bash
./target/release/krx-api-cli stock stk-bydd-trd --help
```

내장 카탈로그 요약/내보내기:

```bash
./target/release/krx-api-cli catalog summary
./target/release/krx-api-cli catalog export --compact
```

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
