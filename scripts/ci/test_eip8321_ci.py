"""Regression checks for fixture integrity and visible CI failures."""
import hashlib
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


vectors = load("vectors", "testing/ef_tests/prepare_ci_vectors.py")
reports = load("reports", "scripts/ci/summarize-eip8321.py")


class CiTests(unittest.TestCase):
    def test_checksum_rejects_changed_archive(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "archive"
            path.write_bytes(b"original")
            digest = hashlib.sha256(b"original").hexdigest()
            vectors.verify_checksum(path, digest)
            path.write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
                vectors.verify_checksum(path, digest)

    def make_archive(self, path, names):
        with tarfile.open(path, "w:gz") as archive:
            for name in names:
                member = tarfile.TarInfo(name)
                member.size = 1
                archive.addfile(member, io.BytesIO(b"x"))

    def test_rejects_archive_traversal(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.make_archive(root / "bad.tar.gz", ["../escape"])
            with self.assertRaisesRegex(ValueError, "Unsafe archive member"):
                vectors.extract(root / "bad.tar.gz", root / "output")
            self.assertFalse((root / "escape").exists())

    def test_compliance_extraction_is_scoped(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.make_archive(root / "tests.tar.gz", ["tests/minimal/gloas/fork_choice_compliance/case.yaml", "source.py"])
            vectors.extract(root / "tests.tar.gz", root / "output", ["tests/minimal/gloas/fork_choice_compliance"])
            self.assertTrue((root / "output/tests/minimal/gloas/fork_choice_compliance/case.yaml").is_file())
            self.assertFalse((root / "output/source.py").exists())
            with self.assertRaisesRegex(ValueError, "No matching"):
                vectors.extract(root / "tests.tar.gz", root / "other", ["absent"])

    def test_missing_feature_vectors_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(ValueError, "Missing EIP-8321"):
                vectors.audit(Path(directory), "mainnet")

    def test_summary_counts_failures_and_skips(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "junit.xml"
            report.write_text('<testsuites><testsuite><testcase name="pass"/><testcase name="bad|name"><failure/></testcase><testcase name="error"><error/></testcase><testcase name="skip"><skipped/></testcase></testsuite></testsuites>')
            text = reports.summary(report)
            self.assertIn("4 tests: 1 passed, 2 failed, 1 skipped", text)
            self.assertIn("bad\\|name", text)

    def test_missing_report_does_not_claim_success(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertIn("No JUnit report", reports.summary(Path(directory) / "missing.xml"))


if __name__ == "__main__":
    unittest.main()
