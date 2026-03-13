# LLM Guide

이 문서는 사람보다 LLM/에이전트가 빠르게 읽고 `krx-api-cli`를 호출할 수 있게
구성한 운영 가이드입니다.

## 우선 읽을 순서

1. 이 문서 `docs/LLM_GUIDE.md`
2. 전체 명령 목록 `docs/CLI_REFERENCE.md`
3. 실제 파라미터 help `krx-api-cli <category> <api> --help`
4. 기계가 읽는 원본 `data/krx_api_manifest.json`

## 명령 문법

기본 형태:

```text
krx-api-cli [GLOBAL OPTIONS] <category> <api> [api flags...]
```

예외적인 최상위 그룹:

```text
krx-api-cli config <subcommand>
krx-api-cli catalog <subcommand>
```

## 전역 옵션

- `--env <sample|real>`
  - 기본값은 `sample`
- `--config <PATH>`
  - OS 기본 config 대신 특정 TOML 파일을 사용합니다.
- `--format <json|xml>`
  - 기본값은 `json`
- `--compact`
  - JSON 출력을 한 줄로 압축합니다.

## 공통 후처리 옵션

API 서브커맨드에는 다음 client-side 후처리 옵션을 붙일 수 있습니다.

- `--filter <FIELD:OP:VALUE>`
- `--sort-by <FIELD>`
- `--order <asc|desc>`
- `--limit <N>`
- `--select <A,B,...>`

규칙:

- 이 옵션들은 KRX 서버 파라미터가 아니라 CLI 후처리입니다.
- JSON 응답에서만 동작합니다.
- 기본 대상은 `OutBlock_1` 같은 list block입니다.
- 존재하지 않는 필드를 지정하면 `program_error.category=invalid_input`입니다.

`--filter` 연산자:

- `eq`
- `ne`
- `gt`
- `gte`
- `lt`
- `lte`
- `contains`

우선 사용할 alias:

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

## 설정 규칙

- `sample` 환경도 `real` 환경과 마찬가지로 `AUTH_KEY`를 명시해야 합니다.
- `real` 환경은 승인된 실제 `AUTH_KEY`가 필요합니다.
- 공개 샘플 키를 코드나 저장소에 내장하지 않습니다.
- 실제 키 저장 명령:
  - `krx-api-cli config set-auth-key --profile sample --stdin`
  - `krx-api-cli config set-auth-key --profile real --stdin`
- config 경로 확인/초기화:
  - `krx-api-cli config path`
  - `krx-api-cli config init`
- 현재 설정 확인:
  - `krx-api-cli config show`

환경변수 우선순위:

1. `KRX_<ENV>_AUTH_KEY` 또는 `KRX_AUTH_KEY`
2. config 파일 값

대표 환경변수:

- `KRX_ENV`
- `KRX_CONFIG`
- `KRX_AUTH_KEY`
- `KRX_SAMPLE_AUTH_KEY`
- `KRX_REAL_AUTH_KEY`
- `KRX_BASE_URL`
- `KRX_SAMPLE_BASE_URL`
- `KRX_REAL_BASE_URL`
- `KRX_USER_AGENT`

## 권장 탐색 절차

1. `krx-api-cli catalog summary --compact`
2. 필요한 카테고리 찾기
3. `krx-api-cli <category> --help`
4. 대상 API 선택
5. `krx-api-cli <category> <api> --help`
6. 필수 플래그를 채워 실행

## 작업별 명령 매핑

### 설정

- config 경로 확인: `krx-api-cli config path`
- config 템플릿 생성: `krx-api-cli config init`
- 실제 인증키 저장: `krx-api-cli config set-auth-key --profile real --stdin`
- redacted config 확인: `krx-api-cli config show`

### 카탈로그 탐색

- 카테고리/개수 요약: `krx-api-cli catalog summary`
- 전체 manifest JSON 출력: `krx-api-cli catalog export`

### 지수

- KRX 시리즈 일별시세정보: `krx-api-cli index krx-dd-trd`
- KOSPI 시리즈 일별시세정보: `krx-api-cli index kospi-dd-trd`
- KOSDAQ 시리즈 일별시세정보: `krx-api-cli index kosdaq-dd-trd`

### 주식

- 유가증권 일별매매정보: `krx-api-cli stock stk-bydd-trd`
- 유가증권 종목기본정보: `krx-api-cli stock stk-isu-base-info`
- 코스닥 일별매매정보: `krx-api-cli stock ksq-bydd-trd`

예시:

```bash
krx-api-cli --env real stock stk-bydd-trd --bas-dd 20260312 \
  --sort-by market_cap --order desc --limit 10 --select name,symbol,market_cap
```

```bash
krx-api-cli --env real stock stk-bydd-trd --bas-dd 20260312 \
  --filter change_rate:gte:20 --sort-by change_rate --order desc \
  --select name,symbol,change_rate,market_cap
```

## 파라미터 규칙

- CLI 플래그 이름은 원본 파라미터의 camelCase 또는 snake_case를 kebab-case로 바꾼 형태입니다.
- 현재 KRX 명세 범위에서는 모든 API가 `--bas-dd <YYYYMMDD>`를 필수로 받습니다.
- `basDd` 형식은 8자리 숫자 `YYYYMMDD`입니다.

## 출력 규칙

- 성공 시:
  - `json`이면 stdout에 JSON 본문 출력
  - `xml`이면 stdout에 XML 문자열 출력
- 실패 시 stderr는 JSON envelope입니다.
- 로컬 입력 검증 오류는 가능한 경우 인증키 확인이나 네트워크 호출보다 먼저 반환됩니다.

오류 분류 규칙:

- `api_error`
  - KRX가 HTTP 오류를 반환한 경우
  - HTTP 200이어도 본문에 KRX 오류 코드/메시지가 있으면 여기에 포함됩니다.
- `program_error`
  - CLI 입력 오류, config 문제, 네트워크 실패, 파일 I/O 문제, 내부 처리 실패

LLM이 우선 읽을 필드:

- `error_type`
- `message`
- `llm_hint.summary`
- `llm_hint.retryable`
- `llm_hint.next_action`
- `api_error.category`
- `api_error.http_status`
- `api_error.code`
- `api_error.message`
- `program_error.category`
- `program_error.suggested_commands`
- `program_error.suggested_env_vars`

LLM 권장 처리 순서:

1. `error_type` 확인
2. `llm_hint.retryable` 확인
3. `api_error.category` 또는 `program_error.category` 확인
4. `llm_hint.next_action` 적용
5. `program_error.suggested_commands` / `program_error.suggested_env_vars`가 있으면 우선 사용

대표 category 해석:

- `api_error.auth_or_permission`
  - 인증키, 승인 상태, 권한 문제입니다.
- `api_error.rate_limited`
  - 잠시 대기 후 같은 요청을 재시도합니다.
- `api_error.invalid_response_format`
  - `--format xml`로 원문 확인 후 대응합니다.
- `program_error.missing_auth_key`
  - config 또는 환경변수에 키를 먼저 설정해야 합니다.
- `program_error.invalid_input`
  - 명령 인자나 파라미터 형식을 수정해야 합니다.
- `program_error.config_error`
  - config 경로, 파일 내용, 권한을 먼저 점검해야 합니다.

## 피해야 할 가정

- raw URL을 직접 조립해서 호출하는 전용 도구라고 가정하지 않습니다.
- `real` 환경에서도 샘플 `AUTH_KEY`가 동작한다고 가정하지 않습니다.
- 모든 API가 같은 카테고리 이름을 쓰는 것은 아니므로 `catalog`나 `--help`를 먼저 확인합니다.
- `sample`과 `real`의 엔드포인트 path는 동일하지 않습니다.
  - `sample`: `/svc/sample/apis/...`
  - `real`: `/svc/apis/...`
