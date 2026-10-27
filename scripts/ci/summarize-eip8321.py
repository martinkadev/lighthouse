#!/usr/bin/env python3
"""Write a nextest JUnit pass/fail table to the GitHub job summary."""
import os
from pathlib import Path
import sys
import xml.etree.ElementTree as ET


def summary(report):
    if not report.is_file():
        return "No JUnit report was produced. Check setup, vector download, or compilation logs.\n"
    cases = list(ET.parse(report).getroot().iter("testcase"))
    rows = []
    failed = skipped = 0
    for case in cases:
        bad = case.find("failure") is not None or case.find("error") is not None
        skip = case.find("skipped") is not None
        failed += bad
        skipped += skip
        status = "FAIL" if bad else "SKIP" if skip else "PASS"
        name = case.get("name", "unknown").replace("|", "\\|").replace("\n", " ").replace("`", "")
        rows.append(f"| {status} | `{name}` |")
    return (f"**{len(cases)} tests: {len(cases)-failed-skipped} passed, {failed} failed, {skipped} skipped.**\n\n"
            "Vector-level known/BLS skips are recorded inside the test output in the JUnit artifact.\n\n"
            "| Result | Test |\n| --- | --- |\n" + "\n".join(rows) + "\n")


if __name__ == "__main__":
    content = summary(Path(sys.argv[1]))
    destination = os.environ.get("GITHUB_STEP_SUMMARY")
    if destination:
        with open(destination, "a") as output:
            output.write(content)
    else:
        print(content)
