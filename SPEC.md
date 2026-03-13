# SPEC.md

## Objective

KRX Open API를 브라우저 샘플 페이지 없이 직접 호출할 수 있는 설치형 Rust CLI를 만든다.
초기 목표는 OPPINFO004 서비스 목록에 노출된 조회형 REST API를 먼저 안정적으로 제공하는 것이다.

## Product Requirements

### Functional

- 사용자는 단일 바이너리로 KRX REST API를 호출할 수 있어야 한다.
- 사용자는 `sample` / `real` 환경을 명시적으로 전환할 수 있어야 한다.
- 사용자는 설정 파일 생성, 인증키 저장, 카탈로그 탐색을 CLI에서 바로 수행할 수 있어야 한다.
- 사용자는 공식 서비스 목록 기준 API 기능이 카테고리별 CLI 서브커맨드로 노출된 도움말을 볼 수 있어야 한다.
- LLM은 `README`, `docs/LLM_GUIDE.md`, `docs/CLI_REFERENCE.md`, embedded manifest를 통해 사용 가능한 기능과 파라미터를 빠르게 파악할 수 있어야 한다.

### Non-Functional

- macOS, Linux, Windows에서 동일한 명령 구조를 유지한다.
- Python 런타임에 의존하지 않는 주 실행 바이너리를 제공한다.
- 민감한 실제 인증키는 저장소 바깥 경로에 저장한다.
- TLS는 `rustls` 기반으로 구성한다.

## Phase 1 Scope

### Included

- Rust binary crate 초기화
- OS별 config 경로 사용
- config template 생성
- GitHub Releases용 설치 스크립트
- GitHub Actions prebuilt build/release workflow
- KRX 서비스 목록/명세 기반 API manifest 생성
- 카테고리별 동적 CLI 도움말/명령 트리
- 사람용 README와 LLM 전용 운영 문서 분리
- manifest 기반 전체 CLI reference 생성
- structured JSON 오류 출력
- manifest 기반 REST executor

### Excluded

- 주문/쓰기 API
- 자동 재시도와 rate-limit 백오프
- 인증키 암호화 저장
