#!/usr/bin/env python3
"""Generate the minimal Gloas filtering vectors missing from older releases.

Uses an explicit local consensus-specs checkout and its installed Python runtime.
Existing vectors are never overwritten. The source revision is recorded with the output.
"""

import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--specs-dir", type=Path, required=True)
    parser.add_argument("--python", type=Path)
    parser.add_argument("--output-dir", type=Path, default=Path(__file__).resolve().parent /
        "consensus-spec-tests/tests/minimal/gloas/fork_choice/filter_node_tree_variants")
    args = parser.parse_args()
    specs = args.specs_dir.resolve()
    destination = args.output_dir.resolve()
    if destination.exists():
        parser.error(f"refusing to overwrite existing vectors: {destination}")
    python = args.python or specs / ".venv/bin/python"
    source_test = "tests/core/pyspec/eth_consensus_specs/test/gloas/fork_choice/test_filter_node_tree_variants.py"
    if not python.is_file() or not (specs / source_test).is_file():
        parser.error("the checkout must contain the test module and an installed Python environment")
    revision = subprocess.check_output(["git", "-C", str(specs), "rev-parse", "HEAD"], text=True).strip()
    with tempfile.TemporaryDirectory(prefix="lighthouse-gloas-vectors-") as directory:
        subprocess.run([str(python), "-m", "pytest", source_test, "--preset", "minimal",
            "--fork", "gloas", "--reftests", "--reftests-output", directory, "-q"],
            cwd=specs, check=True)
        generated = Path(directory) / "minimal/gloas/fork_choice/filter_node_tree_variants"
        cases = [case for case in (generated / "pyspec_tests").iterdir() if case.is_dir()]
        if not cases:
            raise RuntimeError("the generator produced no filtering cases")
        for case in cases:
            for name in ["anchor_state.ssz_snappy", "anchor_block.ssz_snappy", "steps.yaml"]:
                if not (case / name).is_file():
                    raise RuntimeError(f"incomplete generated case: {case / name}")
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(generated, destination)
        (destination / "generation_provenance.json").write_text(json.dumps({
            "source_revision": revision, "source_test": source_test,
            "preset": "minimal", "fork": "gloas", "cases": len(cases),
        }, indent=2) + "\n")
    print(f"Installed {len(cases)} filtering cases from consensus-specs {revision}")


if __name__ == "__main__":
    main()
