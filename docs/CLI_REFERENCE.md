# CLI Reference

> Generated from `data/krx_api_manifest.json`. Edit the manifest or generator, not this file.

- Service list: `https://openapi.krx.co.kr/contents/OPP/INFO/service/OPPINFO004.cmd`
- Spec directory: `krx_docs/specs`
- Categories: `7`
- APIs: `31`

## Top-level commands

- `doctor`: local readiness and config diagnostics
- `config`: local config, encrypted AUTH_KEY, and key-state management
- `catalog`: embedded manifest summary/export
- `index`: KRX OPEN API의 지수 서비스를 제공합니다. (5 APIs)
- `stock`: KRX OPEN API의 주식 서비스를 제공합니다. (8 APIs)
- `etp`: KRX OPEN API의 ETF/ETN/ELW 서비스를 제공합니다. (3 APIs)
- `bond`: KRX OPEN API의 채권 서비스를 제공합니다. (3 APIs)
- `derivatives`: KRX OPEN API의 파생상품 서비스를 제공합니다. (6 APIs)
- `general`: KRX OPEN API의 일반상품 서비스를 제공합니다. (3 APIs)
- `esg`: KRX OPEN API의 ESG 서비스를 제공합니다. (3 APIs)

## Global options

- `--env <sample|real>`
- `--config <PATH>`
- `--format <json|xml>`
- `--compact`

## `index`

- Label: `지수`
- Description: KRX OPEN API의 지수 서비스를 제공합니다.
- API count: `5`

| Command | 설명 | Method | Path | Required flags |
| --- | --- | --- | --- | ---: |
| `bon-dd-trd` | 채권지수 시세정보 | `GET` | `/svc/apis/idx/bon_dd_trd` | 1 |
| `drvprod-dd-trd` | 파생상품지수 시세정보 | `GET` | `/svc/apis/idx/drvprod_dd_trd` | 1 |
| `kosdaq-dd-trd` | KOSDAQ 시리즈 일별시세정보 | `GET` | `/svc/apis/idx/kosdaq_dd_trd` | 1 |
| `kospi-dd-trd` | KOSPI 시리즈 일별시세정보 | `GET` | `/svc/apis/idx/kospi_dd_trd` | 1 |
| `krx-dd-trd` | KRX 시리즈 일별시세정보 | `GET` | `/svc/apis/idx/krx_dd_trd` | 1 |

## `stock`

- Label: `주식`
- Description: KRX OPEN API의 주식 서비스를 제공합니다.
- API count: `8`

| Command | 설명 | Method | Path | Required flags |
| --- | --- | --- | --- | ---: |
| `knx-bydd-trd` | 코넥스 일별매매정보 | `GET` | `/svc/apis/sto/knx_bydd_trd` | 1 |
| `knx-isu-base-info` | 코넥스 종목기본정보 | `GET` | `/svc/apis/sto/knx_isu_base_info` | 1 |
| `ksq-bydd-trd` | 코스닥 일별매매정보 | `GET` | `/svc/apis/sto/ksq_bydd_trd` | 1 |
| `ksq-isu-base-info` | 코스닥 종목기본정보 | `GET` | `/svc/apis/sto/ksq_isu_base_info` | 1 |
| `sr-bydd-trd` | 신주인수권증서 일별매매정보 | `GET` | `/svc/apis/sto/sr_bydd_trd` | 1 |
| `stk-bydd-trd` | 유가증권 일별매매정보 | `GET` | `/svc/apis/sto/stk_bydd_trd` | 1 |
| `stk-isu-base-info` | 유가증권 종목기본정보 | `GET` | `/svc/apis/sto/stk_isu_base_info` | 1 |
| `sw-bydd-trd` | 신주인수권증권 일별매매정보 | `GET` | `/svc/apis/sto/sw_bydd_trd` | 1 |

## `etp`

- Label: `증권상품`
- Description: KRX OPEN API의 ETF/ETN/ELW 서비스를 제공합니다.
- API count: `3`

| Command | 설명 | Method | Path | Required flags |
| --- | --- | --- | --- | ---: |
| `elw-bydd-trd` | ELW 일별매매정보 | `GET` | `/svc/apis/etp/elw_bydd_trd` | 1 |
| `etf-bydd-trd` | ETF 일별매매정보 | `GET` | `/svc/apis/etp/etf_bydd_trd` | 1 |
| `etn-bydd-trd` | ETN 일별매매정보 | `GET` | `/svc/apis/etp/etn_bydd_trd` | 1 |

## `bond`

- Label: `채권`
- Description: KRX OPEN API의 채권 서비스를 제공합니다.
- API count: `3`

| Command | 설명 | Method | Path | Required flags |
| --- | --- | --- | --- | ---: |
| `bnd-bydd-trd` | 일반채권시장 일별매매정보 | `GET` | `/svc/apis/bon/bnd_bydd_trd` | 1 |
| `kts-bydd-trd` | 국채전문유통시장 일별매매정보 | `GET` | `/svc/apis/bon/kts_bydd_trd` | 1 |
| `smb-bydd-trd` | 소액채권시장 일별매매정보 | `GET` | `/svc/apis/bon/smb_bydd_trd` | 1 |

## `derivatives`

- Label: `파생상품`
- Description: KRX OPEN API의 파생상품 서비스를 제공합니다.
- API count: `6`

| Command | 설명 | Method | Path | Required flags |
| --- | --- | --- | --- | ---: |
| `eqkfu-ksq-bydd-trd` | 주식선물(코스닥) 일별매매정보 | `GET` | `/svc/apis/drv/eqkfu_ksq_bydd_trd` | 1 |
| `eqkop-bydd-trd` | 주식옵션(코스닥) 일별매매정보 | `GET` | `/svc/apis/drv/eqkop_bydd_trd` | 1 |
| `eqsfu-stk-bydd-trd` | 주식선물(유가) 일별매매정보 | `GET` | `/svc/apis/drv/eqsfu_stk_bydd_trd` | 1 |
| `eqsop-bydd-trd` | 주식옵션(유가) 일별매매정보 | `GET` | `/svc/apis/drv/eqsop_bydd_trd` | 1 |
| `fut-bydd-trd` | 선물 일별매매정보 (주식선물外) | `GET` | `/svc/apis/drv/fut_bydd_trd` | 1 |
| `opt-bydd-trd` | 옵션 일별매매정보 (주식옵션外) | `GET` | `/svc/apis/drv/opt_bydd_trd` | 1 |

## `general`

- Label: `일반상품`
- Description: KRX OPEN API의 일반상품 서비스를 제공합니다.
- API count: `3`

| Command | 설명 | Method | Path | Required flags |
| --- | --- | --- | --- | ---: |
| `ets-bydd-trd` | 배출권 시장 일별매매정보 | `GET` | `/svc/apis/gen/ets_bydd_trd` | 1 |
| `gold-bydd-trd` | 금시장 일별매매정보 | `GET` | `/svc/apis/gen/gold_bydd_trd` | 1 |
| `oil-bydd-trd` | 석유시장 일별매매정보 | `GET` | `/svc/apis/gen/oil_bydd_trd` | 1 |

## `esg`

- Label: `ESG`
- Description: KRX OPEN API의 ESG 서비스를 제공합니다.
- API count: `3`

| Command | 설명 | Method | Path | Required flags |
| --- | --- | --- | --- | ---: |
| `esg-etp-info` | ESG 증권상품 | `GET` | `/svc/apis/esg/esg_etp_info` | 1 |
| `esg-index-info` | ESG 지수 | `GET` | `/svc/apis/esg/esg_index_info` | 1 |
| `sri-bond-info` | 사회책임투자채권 정보 | `GET` | `/svc/apis/esg/sri_bond_info` | 1 |

