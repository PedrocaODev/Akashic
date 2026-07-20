#!/usr/bin/env python3
"""Dependency-free Markdown and bootstrap OpenSpec structural checks."""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[1]
ARTIFACT = ROOT / "openspec/changes/archive/2026-07-20-bootstrap-rust-harness/parsed-requirements.json"
LINK = re.compile(r"!?(?:\[[^\]]*\])\(([^)]+)\)")
REQUIRED = (
    "akashic.local",
    '"version":1',
    "1048576",
    "lifecycle.shutdown_timeout",
    "SO_PEERCRED",
)


def markdown_files() -> list[Path]:
    return sorted(
        path
        for path in ROOT.rglob("*.md")
        if ".git" not in path.parts and "target" not in path.parts
    )


def check_links() -> list[str]:
    errors: list[str] = []
    for source in markdown_files():
        for raw in LINK.findall(source.read_text(encoding="utf-8")):
            target = raw.strip().split()[0].strip("<>")
            parsed = urlsplit(target)
            if parsed.scheme or parsed.netloc or target.startswith("#"):
                continue
            relative = parsed.path
            if not relative:
                continue
            resolved = (source.parent / relative).resolve()
            if not resolved.exists():
                errors.append(f"{source.relative_to(ROOT)}: missing link {target}")
    return errors


def check_openspec() -> list[str]:
    if ARTIFACT.exists():
        try:
            document = json.loads(ARTIFACT.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            return [f"retained parsed requirements artifact is invalid: {error}"]
        return check_parsed_document(document)
    try:
        result = subprocess.run(
            ["openspec", "show", "bootstrap-rust-harness", "--json"],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        )
    except FileNotFoundError:
        return ["openspec CLI is required for parsed requirement validation"]
    except subprocess.CalledProcessError as error:
        return [f"openspec show failed: {error.stderr.strip()}"]
    try:
        document = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        return [f"openspec show returned invalid JSON: {error}"]

    return check_parsed_document(document)


def check_parsed_document(document: dict) -> list[str]:
    errors: list[str] = []
    deltas = document.get("deltas", [])
    if not deltas:
        errors.append("bootstrap change has no parsed requirement deltas")
    bodies: list[str] = []
    for index, delta in enumerate(deltas):
        requirement = delta.get("requirement", {})
        requirements = [requirement, *delta.get("requirements", [])]
        for requirement_index, body in enumerate(requirements):
            text = body.get("text", "") if isinstance(body, dict) else ""
            scenarios = body.get("scenarios", []) if isinstance(body, dict) else None
            label = f"parsed delta {index} requirement {requirement_index}"
            if not text.strip() or len(text.strip()) < 20:
                errors.append(f"{label} has an incomplete requirement body")
            if not isinstance(scenarios, list) or not scenarios:
                errors.append(f"{label} has no complete scenarios")
            else:
                for scenario_index, scenario in enumerate(scenarios):
                    raw = scenario.get("rawText", "") if isinstance(scenario, dict) else ""
                    if not re.search(r"\bWHEN\b", raw) or not re.search(r"\bTHEN\b", raw):
                        errors.append(
                            f"{label} scenario {scenario_index} must contain WHEN and THEN"
                        )
            bodies.append(text)
    joined = "\n".join(bodies)
    for token in REQUIRED:
        if token not in joined:
            errors.append(f"parsed bootstrap requirements missing {token!r}")
    return errors


def check_untracked_whitespace() -> list[str]:
    result = subprocess.run(
        ["git", "status", "--porcelain=v1", "-z"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    errors: list[str] = []
    for entry in result.stdout.split(b"\0"):
        if not entry.startswith(b"?? "):
            continue
        path = ROOT / entry[3:].decode()
        if not path.is_file():
            continue
        for line_number, line in enumerate(path.read_bytes().splitlines(), 1):
            if line.endswith((b" ", b"\t")):
                errors.append(f"{path.relative_to(ROOT)}:{line_number}: trailing whitespace")
    return errors


def main() -> int:
    errors = check_links() + check_openspec() + check_untracked_whitespace()
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"documentation checks passed: {len(markdown_files())} Markdown files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
