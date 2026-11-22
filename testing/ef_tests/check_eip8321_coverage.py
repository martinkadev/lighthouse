#!/usr/bin/env python3
"""Require every executable published EIP-8321 case to pass in a suitable crypto run."""
import argparse
import json
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("preset", choices=["mainnet", "minimal"])
parser.add_argument("--fixtures", type=Path, default=Path(__file__).parent / "consensus-spec-tests/tests")
parser.add_argument("results", nargs="+", type=Path)
args = parser.parse_args()
root = args.fixtures / args.preset / "eip8321"
if not root.is_dir():
    parser.error(f"Missing EIP-8321 fixture directory: {root}")
expected = set()
placeholders = []
for case in root.glob("*/*/*/*"):
    if not case.is_dir():
        continue
    names = {entry.name for entry in case.iterdir()}
    key = case.relative_to(args.fixtures).as_posix()
    if names == {"manifest.yaml"}:
        placeholders.append(key)
    elif names:
        expected.add(key)
if not expected:
    parser.error("No executable fixtures discovered")
passed = set()
bad = []
for result_file in args.results:
    for line_number, line in enumerate(result_file.read_text().splitlines(), 1):
        result = json.loads(line)
        key, status = result["case"], result["status"]
        if not key.startswith(args.preset + "/eip8321/"):
            continue
        if status == "passed":
            passed.add(key)
        elif status != "skipped_bls":
            bad.append(f"{key}: {status} ({result_file}:{line_number})")
missing = sorted(expected - passed)
for key in missing:
    print(f"MISSING PASS: {key}")
for failure in bad:
    print(f"FAILED OR EXCLUDED: {failure}")
for key in sorted(placeholders):
    print(f"MANIFEST ONLY (no executable inputs): {key}")
print(f"{len(expected - set(missing))}/{len(expected)} executable cases passed; {len(missing)} missing; {len(bad)} failures/exclusions; {len(placeholders)} manifest-only declarations")
raise SystemExit(1 if missing or bad else 0)
