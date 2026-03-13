#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import asdict, dataclass
from html import unescape
from pathlib import Path
from typing import Iterable
from urllib.parse import urlencode, urljoin
from urllib.request import Request, urlopen


BASE_URL = "https://openapi.krx.co.kr"
SERVICE_LIST_URL = (
    "https://openapi.krx.co.kr/contents/OPP/INFO/service/OPPINFO004.cmd"
)
DOWNLOAD_PATH = "/contents/OPP/USES/service/downloadApiDoc.cmd"
ROOT = Path(__file__).resolve().parents[1]
SPECS_DIR = ROOT / "krx_docs" / "specs"
CATALOG_PATH = ROOT / "krx_docs" / "service_catalog.json"
USER_AGENT = "krx-api-cli-spec-sync/0.1"

SERVICE_LINK_RE = re.compile(
    r'<td><a href="(?P<href>/contents/OPP/USES/service/OPPUSES\d+_S2\.cmd\?BO_ID=[^"]+)"'
    r' class="link">(?P<title>.*?)</a></td>\s*<td>(?P<description>.*?)</td>',
    re.S,
)
INPUT_RE = re.compile(
    r'<input[^>]+name="(?P<name>[^"]+)"[^>]+value="(?P<value>[^"]*)"',
    re.S,
)
DATE_RE = re.compile(r"<dt>최근 수정일</dt>\s*<dd>(?P<date>[^<]+)</dd>", re.S)
NAME_RE = re.compile(r"<dt class=\"bdT\">API 명</dt>\s*<dd[^>]*>(?P<name>[^<]+)</dd>", re.S)
DESC_RE = re.compile(r"<dt>설명</dt>\s*<dd[^>]*>(?P<desc>[^<]+)</dd>", re.S)
API_URL_RE = re.compile(
    r'<input[^>]+name="apiUrl"[^>]+value="https://data-dbg\.krx\.co\.kr(?P<path>/svc/sample/apis/[^"]+)"',
    re.S,
)


@dataclass
class ServiceEntry:
    title: str
    description: str
    detail_url: str


@dataclass
class DownloadEntry:
    title: str
    description: str
    detail_url: str
    path: str
    bo_id: str
    bo_ver: str
    api_id: str
    api_path: str
    modified_at: str | None
    source_file: str
    status: str


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Sync KRX API docx specs using the official '개발 명세서 다운로드' flow."
    )
    parser.add_argument(
        "--missing-only",
        action="store_true",
        help="Only download missing docx files; do not refresh existing ones.",
    )
    parser.add_argument(
        "--catalog-out",
        default=str(CATALOG_PATH),
        help="Output path for downloaded service metadata JSON.",
    )
    args = parser.parse_args()

    services = fetch_service_list()
    results = sync_specs(services, missing_only=args.missing_only)
    catalog_path = Path(args.catalog_out).expanduser().resolve()
    catalog_path.parent.mkdir(parents=True, exist_ok=True)
    catalog_path.write_text(
        json.dumps(
            {
                "service_list_url": SERVICE_LIST_URL,
                "service_count": len(results),
                "entries": [asdict(entry) for entry in results],
            },
            ensure_ascii=False,
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )

    summary = summarize(results)
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    return 0


def fetch_service_list() -> list[ServiceEntry]:
    html = fetch_text(SERVICE_LIST_URL)
    services: list[ServiceEntry] = []
    for match in SERVICE_LINK_RE.finditer(html):
        href = unescape(match.group("href"))
        title = clean_text(match.group("title"))
        description = clean_text(match.group("description"))
        services.append(
            ServiceEntry(
                title=title,
                description=description,
                detail_url=urljoin(BASE_URL, href),
            )
        )

    if not services:
        raise RuntimeError("failed to parse service list page")

    return services


def sync_specs(services: Iterable[ServiceEntry], missing_only: bool) -> list[DownloadEntry]:
    SPECS_DIR.mkdir(parents=True, exist_ok=True)
    results: list[DownloadEntry] = []

    for service in services:
        detail_html = fetch_text(service.detail_url)
        inputs = dict(INPUT_RE.findall(detail_html))
        if not {"path", "BO_ID", "BO_VER"} <= set(inputs):
            raise RuntimeError(f"failed to parse download form for {service.detail_url}")

        api_id = inputs.get("apiId")
        if not api_id:
            api_path_match = API_URL_RE.search(detail_html)
            if not api_path_match:
                raise RuntimeError(f"failed to parse apiId for {service.detail_url}")
            api_id = api_path_match.group("path").rsplit("/", 1)[-1]

        api_path = extract_api_path(detail_html)
        modified_at = extract_optional(DATE_RE, detail_html, "date")
        resolved_title = extract_optional(NAME_RE, detail_html, "name") or service.title
        resolved_description = (
            extract_optional(DESC_RE, detail_html, "desc") or service.description
        )

        docx_path = SPECS_DIR / f"{api_id}.docx"
        if missing_only and docx_path.exists():
            status = "skipped_existing"
        else:
            payload = urlencode(
                {
                    "path": inputs["path"],
                    "BO_ID": inputs["BO_ID"],
                    "BO_VER": inputs["BO_VER"],
                }
            ).encode("utf-8")
            docx_bytes = fetch_bytes(urljoin(BASE_URL, DOWNLOAD_PATH), data=payload)

            if docx_path.exists():
                existing_bytes = docx_path.read_bytes()
                if existing_bytes == docx_bytes:
                    status = "unchanged"
                else:
                    docx_path.write_bytes(docx_bytes)
                    status = "updated"
            else:
                docx_path.write_bytes(docx_bytes)
                status = "downloaded"

        results.append(
            DownloadEntry(
                title=resolved_title,
                description=resolved_description,
                detail_url=service.detail_url,
                path=inputs["path"],
                bo_id=inputs["BO_ID"],
                bo_ver=inputs["BO_VER"],
                api_id=api_id,
                api_path=api_path,
                modified_at=modified_at,
                source_file=str(docx_path.relative_to(ROOT)),
                status=status,
            )
        )

    return results


def extract_api_path(detail_html: str) -> str:
    api_url_match = API_URL_RE.search(detail_html)
    if not api_url_match:
        raise RuntimeError("failed to parse sample api path from detail page")
    return api_url_match.group("path").replace("/svc/sample/apis/", "/svc/apis/")


def extract_optional(pattern: re.Pattern[str], text: str, group: str) -> str | None:
    match = pattern.search(text)
    if not match:
        return None
    return clean_text(match.group(group))


def fetch_text(url: str) -> str:
    return fetch_bytes(url).decode("utf-8", errors="replace")


def fetch_bytes(url: str, data: bytes | None = None) -> bytes:
    request = Request(
        url,
        data=data,
        headers={
            "User-Agent": USER_AGENT,
            "Content-Type": "application/x-www-form-urlencoded",
        },
        method="POST" if data is not None else "GET",
    )

    with urlopen(request, timeout=30) as response:
        return response.read()


def clean_text(value: str) -> str:
    return re.sub(r"\s+", " ", unescape(value)).strip()


def summarize(results: list[DownloadEntry]) -> dict:
    counts: dict[str, int] = {}
    for result in results:
        counts[result.status] = counts.get(result.status, 0) + 1

    return {
        "service_count": len(results),
        "statuses": counts,
        "catalog_path": str(CATALOG_PATH),
        "specs_dir": str(SPECS_DIR),
    }


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(json.dumps({"ok": False, "message": str(error)}, ensure_ascii=False), file=sys.stderr)
        raise
