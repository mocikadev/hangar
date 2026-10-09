"""Negative checks for the artifact boundary, with tiny non-executable fixtures."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'verify-linux-package.py'
SPEC = importlib.util.spec_from_file_location('verify_linux_package', SCRIPT)
VERIFY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFY)


class ChecksumTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='hangar-package-unit-')
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        content = b'checksum fixture, not an executable or real deb'
        self.archive = self.directory / 'example.deb'
        self.archive.write_bytes(content)
        self.manifest = {'schema_version': 2, 'artifacts': [
            {'path': self.archive.name, 'bytes': len(content), 'sha256': hashlib.sha256(content).hexdigest()}
        ]}
        self.write_manifest()

    def write_manifest(self):
        (self.directory / 'manifest.json').write_text(json.dumps(self.manifest))
        self.sums = self.directory / 'SHA256SUMS.txt'
        self.sums.write_text(''.join(f'{VERIFY.digest(self.directory / name)}  {name}\n'
                                    for name in ['example.deb', 'manifest.json']))

    def test_valid_checksums(self):
        self.assertEqual(VERIFY.checksums(self.directory, self.manifest), self.archive)

    def test_modified_artifact_is_rejected(self):
        self.archive.write_bytes(b'modified')
        with self.assertRaisesRegex(ValueError, 'Checksum mismatch'):
            VERIFY.checksums(self.directory, self.manifest)

    def test_manifest_must_be_covered_and_unmodified(self):
        self.sums.write_text(self.sums.read_text().splitlines()[0] + '\n')
        with self.assertRaisesRegex(ValueError, 'Incomplete checksum coverage'):
            VERIFY.checksums(self.directory, self.manifest)
        self.write_manifest()
        with (self.directory / 'manifest.json').open('a') as file:
            file.write(' ')
        with self.assertRaisesRegex(ValueError, 'Checksum mismatch'):
            VERIFY.checksums(self.directory, self.manifest)

    def test_paths_cannot_escape_artifact_directory(self):
        self.manifest['artifacts'][0]['path'] = '../example.deb'
        with self.assertRaisesRegex(ValueError, 'Invalid artifact path'):
            VERIFY.checksums(self.directory, self.manifest)

    def test_duplicate_checksums_and_size_mismatch_are_rejected(self):
        self.sums.write_text(self.sums.read_text() * 2)
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            VERIFY.checksums(self.directory, self.manifest)
        self.manifest['artifacts'][0]['bytes'] += 1
        self.write_manifest()
        with self.assertRaisesRegex(ValueError, 'size mismatch'):
            VERIFY.checksums(self.directory, self.manifest)


if __name__ == '__main__':
    unittest.main()
