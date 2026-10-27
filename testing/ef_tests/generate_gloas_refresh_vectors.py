#!/usr/bin/env python3
"""Generate replacement Gloas vectors from a local matching spec checkout.

Output must be a fresh directory. This never overwrites installed test vectors.
Keep the generated inputs together with their expectations when replacing a case.
"""
import argparse
import json
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--specs-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--suite", choices=["fast-confirmation", "compliance"], required=True)
    parser.add_argument("--workers", type=int, default=4)
    args = parser.parse_args()
    specs = args.specs_dir.resolve()
    output = args.output_dir.resolve()
    python = specs / ".venv/bin/python"
    if output.exists():
        parser.error(f"refusing to overwrite existing output: {output}")
    if not python.is_file() or args.workers < 1:
        parser.error("an installed spec Python environment and positive worker count are required")
    revision = subprocess.check_output(["git", "-C", str(specs), "rev-parse", "HEAD"], text=True).strip()
    if args.suite == "fast-confirmation":
        module = "tests/core/pyspec/eth_consensus_specs/test/phase0/fast_confirmation/test_is_one_confirmed.py"
        command = [str(python), "-m", "pytest", module, "--preset", "minimal", "--fork", "gloas",
            "--reftests", "--reftests-output", str(output),
            "-k", "test_is_one_confirmed_passes_with_empty_slot_and_attester_in_two_consecutive_slots_1", "-q"]
    else:
        module = "tests/generators/compliance_runners/fork_choice/generate_comptests.py"
        command = [str(python), "-m", "pytest", module, "--presets", "minimal", "--forks", "gloas",
            "--fc-gen-config", "small", "--comptests-output", str(output), "-n", str(args.workers), "-q"]
    if not (specs / module).is_file():
        parser.error(f"missing source module: {module}")
    subprocess.run(command, cwd=specs, check=True)
    (output / "generation_provenance.json").write_text(json.dumps({
        "source_revision": revision, "suite": args.suite, "command": command,
        "preset": "minimal", "fork": "gloas",
    }, indent=2) + "\n")


if __name__ == "__main__":
    main()
