#!/usr/bin/env python3
from __future__ import annotations

import json
import re
import sys
import zipfile
from datetime import datetime, timezone
from pathlib import Path
from xml.etree import ElementTree


ROOT = Path(__file__).resolve().parents[1]
SPEC_DIR = ROOT / "krx_docs" / "specs"
CATALOG_FILE = ROOT / "krx_docs" / "service_catalog.json"
OUT_FILE = ROOT / "data" / "krx_api_manifest.json"
DOC_NS = {"w": "http://schemas.openxmlformats.org/wordprocessingml/2006/main"}

CATEGORY_ORDER = ["index", "stock", "etp", "bond", "derivatives", "general", "esg"]
CATEGORY_MAP = {
    "idx": {
        "id": "index",
        "path_segment": "idx",
        "label": "지수",
        "description": "KRX OPEN API의 지수 서비스를 제공합니다.",
    },
    "sto": {
        "id": "stock",
        "path_segment": "sto",
        "label": "주식",
        "description": "KRX OPEN API의 주식 서비스를 제공합니다.",
    },
    "etp": {
        "id": "etp",
        "path_segment": "etp",
        "label": "증권상품",
        "description": "KRX OPEN API의 ETF/ETN/ELW 서비스를 제공합니다.",
    },
    "bon": {
        "id": "bond",
        "path_segment": "bon",
        "label": "채권",
        "description": "KRX OPEN API의 채권 서비스를 제공합니다.",
    },
    "drv": {
        "id": "derivatives",
        "path_segment": "drv",
        "label": "파생상품",
        "description": "KRX OPEN API의 파생상품 서비스를 제공합니다.",
    },
    "gen": {
        "id": "general",
        "path_segment": "gen",
        "label": "일반상품",
        "description": "KRX OPEN API의 일반상품 서비스를 제공합니다.",
    },
    "esg": {
        "id": "esg",
        "path_segment": "esg",
        "label": "ESG",
        "description": "KRX OPEN API의 ESG 서비스를 제공합니다.",
    },
}


def main() -> int:
    manifest, warnings = build_manifest()
    OUT_FILE.parent.mkdir(parents=True, exist_ok=True)
    OUT_FILE.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    for warning in warnings:
        print(f"warning: {warning}", file=sys.stderr)
    print(f"wrote {OUT_FILE}")
    return 0


def build_manifest() -> tuple[dict, list[str]]:
    warnings: list[str] = []
    catalog_entries = load_catalog_entries(warnings)
    spec_paths = sorted(SPEC_DIR.glob("*.docx"))
    spec_paths_by_api_id = {path.stem: path for path in spec_paths}

    if catalog_entries is None:
        apis = [build_api_entry(path, None, warnings) for path in spec_paths]
        source = {
            "service_list_url": "https://openapi.krx.co.kr/contents/OPP/INFO/service/OPPINFO004.cmd",
            "spec_directory": "krx_docs/specs",
        }
    else:
        catalog_by_api_id = {entry["api_id"]: entry for entry in catalog_entries}
        missing_specs = sorted(set(catalog_by_api_id) - set(spec_paths_by_api_id))
        extra_specs = sorted(set(spec_paths_by_api_id) - set(catalog_by_api_id))

        if missing_specs:
            warnings.append(
                "catalog contains APIs without local docx specs: "
                + ", ".join(missing_specs)
                + ". Run `python3 tools/sync_krx_specs.py`."
            )
        if extra_specs:
            warnings.append(
                "local docx specs are present but not listed in service_catalog.json: "
                + ", ".join(extra_specs)
                + ". Regenerate the catalog with `python3 tools/sync_krx_specs.py`."
            )

        apis = []
        for entry in catalog_entries:
            api_id = entry["api_id"]
            path = spec_paths_by_api_id.get(api_id)
            if path is None:
                continue
            apis.append(build_api_entry(path, entry, warnings))

        source = {
            "service_list_url": "https://openapi.krx.co.kr/contents/OPP/INFO/service/OPPINFO004.cmd",
            "spec_directory": "krx_docs/specs",
            "catalog_file": "krx_docs/service_catalog.json",
            "catalog_service_count": len(catalog_entries),
        }

    category_api_counts: dict[str, int] = {category_id: 0 for category_id in CATEGORY_ORDER}
    for entry in apis:
        category_api_counts[entry["category_id"]] += 1

    categories = []
    for category_id in CATEGORY_ORDER:
        category_source = next(
            value for value in CATEGORY_MAP.values() if value["id"] == category_id
        )
        categories.append(
            {
                "id": category_source["id"],
                "path_segment": category_source["path_segment"],
                "label": category_source["label"],
                "description": category_source["description"],
                "api_count": category_api_counts[category_source["id"]],
            }
        )

    manifest = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "source": source,
        "category_count": len(categories),
        "api_count": len(apis),
        "categories": categories,
        "apis": apis,
    }
    return manifest, warnings


def build_api_entry(path: Path, catalog_entry: dict | None, warnings: list[str]) -> dict:
    lines = docx_lines(path)
    endpoint = find_value(lines, "Server endpoint url : ")
    parsed_description = strip_numbering(find_value_after(lines, "1.2. Description"))
    parsed_title = strip_numbering(find_value_after(lines, "API Spec"))
    params = parse_request_params(lines)
    path_segment = endpoint.split("/svc/apis/")[1].split("/")[0]
    category = CATEGORY_MAP[path_segment]
    api_id = endpoint.rsplit("/", 1)[-1]
    api_path = endpoint.removeprefix("https://data-dbg.krx.co.kr")
    sample_path = endpoint.replace("/svc/apis/", "/svc/sample/apis/").removeprefix(
        "https://data-dbg.krx.co.kr"
    )
    display_name = parsed_title
    description = parsed_description

    if catalog_entry is not None:
        if catalog_entry["api_path"] != api_path:
            warnings.append(
                f"{api_id}: catalog api_path={catalog_entry['api_path']} but docx api_path={api_path}"
            )
        if catalog_entry["path"] != path_segment:
            warnings.append(
                f"{api_id}: catalog path={catalog_entry['path']} but docx path={path_segment}"
            )
        display_name = catalog_entry.get("title") or parsed_title
        description = catalog_entry.get("description") or parsed_description

    entry = {
        "id": f"{category['id']}.{api_id.replace('_', '-')}",
        "category_id": category["id"],
        "path_segment": path_segment,
        "api_id": api_id,
        "command_name": api_id.replace("_", "-"),
        "display_name": display_name,
        "description": description,
        "api_path": api_path,
        "sample_path": sample_path,
        "http_method": "GET",
        "source_file": str(path.relative_to(ROOT)),
        "params": params,
    }
    if catalog_entry is not None:
        entry["detail_url"] = catalog_entry.get("detail_url")
        entry["modified_at"] = catalog_entry.get("modified_at")
        entry["bo_id"] = catalog_entry.get("bo_id")
        entry["bo_ver"] = catalog_entry.get("bo_ver")
    return entry


def load_catalog_entries(warnings: list[str]) -> list[dict] | None:
    if not CATALOG_FILE.exists():
        warnings.append(
            "service_catalog.json not found; generating manifest from local docx files only. "
            "Run `python3 tools/sync_krx_specs.py` to sync the official service catalog first."
        )
        return None

    payload = json.loads(CATALOG_FILE.read_text(encoding="utf-8"))
    entries = payload.get("entries", [])
    if not isinstance(entries, list):
        raise ValueError("invalid service_catalog.json: `entries` must be a list")
    return entries


def docx_lines(path: Path) -> list[str]:
    with zipfile.ZipFile(path) as archive:
        xml = archive.read("word/document.xml")

    root = ElementTree.fromstring(xml)
    paragraphs: list[str] = []
    for paragraph in root.findall(".//w:p", DOC_NS):
        text = "".join(node.text or "" for node in paragraph.findall(".//w:t", DOC_NS))
        text = normalize_text(text)
        if text:
            paragraphs.append(text)
    return paragraphs


def parse_request_params(lines: list[str]) -> list[dict]:
    start = index_of(lines, "1.3.1. InBlock_1")
    end = next(
        index for index, line in enumerate(lines[start + 1 :], start + 1) if line.startswith("1.4.")
    )
    body = [
        line
        for line in lines[start + 1 : end]
        if line not in {"Name", "Type", "Description"}
    ]

    params = []
    for offset in range(0, len(body), 3):
        chunk = body[offset : offset + 3]
        if len(chunk) < 3:
            continue
        name, param_type, description = chunk
        params.append(
            {
                "name": name,
                "cli_name": camel_to_kebab(name),
                "type": param_type,
                "required": True,
                "description": description,
            }
        )

    return params


def index_of(lines: list[str], needle: str) -> int:
    for index, line in enumerate(lines):
        if line == needle:
            return index
    raise ValueError(f"{needle!r} not found")


def find_value(lines: list[str], prefix: str) -> str:
    for line in lines:
        if line.startswith(prefix):
            return line[len(prefix) :].strip()
    raise ValueError(f"{prefix!r} not found")


def find_value_after(lines: list[str], anchor: str) -> str:
    anchor_index = index_of(lines, anchor)
    for line in lines[anchor_index + 1 :]:
        if line:
            return line.strip()
    raise ValueError(f"value after {anchor!r} not found")


def strip_numbering(value: str) -> str:
    return re.sub(r"^\d+\.\d+\.\s*", "", value).strip()


def camel_to_kebab(value: str) -> str:
    value = re.sub(r"([a-z0-9])([A-Z])", r"\1-\2", value)
    return value.replace("_", "-").lower()


def normalize_text(value: str) -> str:
    return re.sub(r"\s+", " ", value).strip()


if __name__ == "__main__":
    raise SystemExit(main())
