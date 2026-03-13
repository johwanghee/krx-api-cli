#!/usr/bin/env python3
from __future__ import annotations

import json
from collections import defaultdict
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MANIFEST_FILE = ROOT / "data" / "krx_api_manifest.json"
OUT_FILE = ROOT / "docs" / "CLI_REFERENCE.md"


def main() -> int:
    manifest = json.loads(MANIFEST_FILE.read_text(encoding="utf-8"))
    markdown = render_reference(manifest)
    OUT_FILE.parent.mkdir(parents=True, exist_ok=True)
    OUT_FILE.write_text(markdown, encoding="utf-8")
    print(f"wrote {OUT_FILE}")
    return 0


def render_reference(manifest: dict) -> str:
    grouped = defaultdict(list)
    for entry in manifest["apis"]:
        grouped[entry["category_id"]].append(entry)

    lines: list[str] = []
    lines.append("# CLI Reference")
    lines.append("")
    lines.append(
        "> Generated from `data/krx_api_manifest.json`. Edit the manifest or generator, not this file."
    )
    lines.append("")
    lines.append(f"- Service list: `{manifest['source']['service_list_url']}`")
    lines.append(f"- Spec directory: `{manifest['source']['spec_directory']}`")
    lines.append(f"- Categories: `{manifest['category_count']}`")
    lines.append(f"- APIs: `{manifest['api_count']}`")
    lines.append("")
    lines.append("## Top-level commands")
    lines.append("")
    lines.append("- `config`: local config, encrypted AUTH_KEY, and key-state management")
    lines.append("- `catalog`: embedded manifest summary/export")
    for category in manifest["categories"]:
        lines.append(
            f"- `{category['id']}`: {category['description']} ({category['api_count']} APIs)"
        )
    lines.append("")
    lines.append("## Global options")
    lines.append("")
    lines.append("- `--env <sample|real>`")
    lines.append("- `--config <PATH>`")
    lines.append("- `--format <json|xml>`")
    lines.append("- `--compact`")
    lines.append("")

    for category in manifest["categories"]:
        lines.append(f"## `{category['id']}`")
        lines.append("")
        lines.append(f"- Label: `{category['label']}`")
        lines.append(f"- Description: {category['description']}")
        lines.append(f"- API count: `{category['api_count']}`")
        lines.append("")
        lines.append("| Command | 설명 | Method | Path | Required flags |")
        lines.append("| --- | --- | --- | --- | ---: |")

        for entry in sorted(grouped[category["id"]], key=lambda item: item["command_name"]):
            lines.append(
                "| "
                f"`{entry['command_name']}` | "
                f"{escape(entry['display_name'])} | "
                f"`{entry['http_method']}` | "
                f"`{entry['api_path']}` | "
                f"{len(entry['params'])} |"
            )
        lines.append("")

    return "\n".join(lines) + "\n"


def escape(value: str) -> str:
    return value.replace("|", "\\|")


if __name__ == "__main__":
    raise SystemExit(main())
