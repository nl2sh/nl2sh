"""Exercise signature, index, package, and archive checks with a disposable key."""
import gzip
import hashlib
import importlib.util
import io
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location('pages_apt', Path(__file__).parents[1] / 'merge-pages-apt.py')
APT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(APT)


class AptSnapshotTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workspace = tempfile.TemporaryDirectory()
        cls.home = Path(cls.workspace.name) / 'gpg'
        cls.home.mkdir(mode=0o700)
        subprocess.run(['gpg', '--homedir', str(cls.home), '--batch', '--pinentry-mode', 'loopback', '--passphrase', '', '--quick-generate-key', 'nl2sh disposable test', 'ed25519', 'sign', '0'], check=True, capture_output=True)
        output = subprocess.check_output(['gpg', '--homedir', str(cls.home), '--batch', '--with-colons', '--list-keys'], text=True)
        cls.fingerprint = next(line.split(':')[9] for line in output.splitlines() if line.startswith('fpr:'))
        cls.key = subprocess.check_output(['gpg', '--homedir', str(cls.home), '--batch', '--export', cls.fingerprint])

    @classmethod
    def tearDownClass(cls):
        subprocess.run(['gpgconf', '--homedir', str(cls.home), '--kill', 'all'], check=True, capture_output=True)
        cls.workspace.cleanup()

    def make_repo(self, root, arches=('aarch64', 'arm', 'x86_64')):
        repo = Path(root) / 'repo'
        stable = repo / 'dists/stable'
        stable.mkdir(parents=True)
        (repo / 'nl2sh-repo.gpg').write_bytes(self.key)
        hashes = []
        for arch in arches:
            name = f'pool/main/n/nl2sh/nl2sh_1.0.0_{arch}.deb'
            package = repo / name
            package.parent.mkdir(parents=True, exist_ok=True)
            package.write_bytes(b'disposable package bytes ' + arch.encode())
            index = (f'Package: nl2sh\nVersion: 1.0.0\nArchitecture: {arch}\nFilename: {name}\nSize: {package.stat().st_size}\nSHA256: {hashlib.sha256(package.read_bytes()).hexdigest()}\n\n').encode()
            for suffix, value in [('Packages', index), ('Packages.gz', gzip.compress(index, mtime=0))]:
                path = stable / f'main/binary-{arch}/{suffix}'
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(value)
                hashes.append(f' {hashlib.sha256(value).hexdigest()} {len(value)} {path.relative_to(stable)}')
        (stable / 'Release').write_text('Codename: stable\nArchitectures: ' + ' '.join(arches) + '\nSHA256:\n' + '\n'.join(hashes) + '\n')
        self.sign(stable)
        return repo

    def sign(self, stable):
        for flag, name in [('--clearsign', 'InRelease'), ('--detach-sign', 'Release.gpg')]:
            subprocess.run(['gpg', '--homedir', str(self.home), '--batch', '--yes', '--pinentry-mode', 'loopback', '--passphrase', '', '--local-user', self.fingerprint, '--output', str(stable / name), flag, str(stable / 'Release')], check=True, capture_output=True)

    def test_valid_signed_repository(self):
        with tempfile.TemporaryDirectory() as tmp:
            APT.verify(self.make_repo(tmp), self.fingerprint)

    def test_legacy_signed_repository(self):
        with tempfile.TemporaryDirectory() as tmp:
            APT.verify(self.make_repo(tmp, ('aarch64', 'arm')), self.fingerprint)

    def test_modified_x86_64_package_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = self.make_repo(tmp)
            next(repo.glob('pool/**/*_x86_64.deb')).write_bytes(b'changed')
            with self.assertRaisesRegex(ValueError, 'SHA-256 mismatch'):
                APT.verify(repo, self.fingerprint)

    def test_missing_x86_64_index_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = self.make_repo(tmp)
            (repo / 'dists/stable/main/binary-x86_64/Packages').unlink()
            with self.assertRaises(FileNotFoundError):
                APT.verify(repo, self.fingerprint)

    def test_unsupported_signed_architecture_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaisesRegex(ValueError, 'architectures'):
                APT.verify(self.make_repo(tmp, ('aarch64', 'arm', 'i686')), self.fingerprint)

    def test_modified_package_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = self.make_repo(tmp)
            next(repo.glob('pool/**/*.deb')).write_bytes(b'changed')
            with self.assertRaisesRegex(ValueError, 'SHA-256 mismatch'):
                APT.verify(repo, self.fingerprint)

    def test_modified_index_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = self.make_repo(tmp)
            (repo / 'dists/stable/main/binary-arm/Packages').write_bytes(b'changed')
            with self.assertRaisesRegex(ValueError, 'SHA-256 mismatch'):
                APT.verify(repo, self.fingerprint)

    def test_unsigned_change_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = self.make_repo(tmp)
            path = repo / 'dists/stable/Release'
            path.write_text(path.read_text().replace('Codename: stable', 'Codename: changed'))
            with self.assertRaises(subprocess.CalledProcessError):
                APT.verify(repo, self.fingerprint)

    def test_wrong_key_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaisesRegex(ValueError, 'fingerprint mismatch'):
                APT.verify(self.make_repo(tmp), '0' * 40)

    def test_signed_path_traversal_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = self.make_repo(tmp)
            stable = repo / 'dists/stable'
            release = stable / 'Release'
            release.write_text(release.read_text() + ' ' + '0' * 64 + ' 0 ../../escape\n')
            self.sign(stable)
            with self.assertRaisesRegex(ValueError, 'unsafe repository path'):
                APT.verify(repo, self.fingerprint)

    def test_archive_links_and_traversal_rejected(self):
        for name, kind in [('../escape', tarfile.REGTYPE), ('pool/link', tarfile.SYMTYPE)]:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as tmp:
                archive = Path(tmp) / 'snapshot.tar.gz'
                with tarfile.open(archive, 'w:gz') as tar:
                    info = tarfile.TarInfo(name)
                    info.type = kind
                    info.linkname = '/etc/passwd' if kind == tarfile.SYMTYPE else ''
                    info.size = 0
                    tar.addfile(info, io.BytesIO(b''))
                with self.assertRaises(ValueError):
                    APT.extract(archive, Path(tmp) / 'out')

if __name__ == '__main__':
    unittest.main()
