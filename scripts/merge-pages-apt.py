#!/usr/bin/env python3
"""Merge a verified signed APT snapshot into a built documentation site.

Use the latest stable Release snapshot, or bootstrap from existing Pages when
older releases have no snapshot. Never publish a docs-only site on failure.
"""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import tarfile
import tempfile
from urllib.parse import quote
from urllib.request import Request, urlopen

FINGERPRINT = '5230D3A7CCBEED4616D39C51FC6AD1BC63F7D4D8'
BASE = 'https://nl2sh.github.io/nl2sh/'
LIMIT = 256 * 1024 * 1024


def relative(name):
    path = PurePosixPath(name)
    if path.is_absolute() or '..' in path.parts or '\\' in name or not path.parts:
        raise ValueError(f'unsafe repository path: {name}')
    return path


def fetch(url, target, headers=None):
    request = Request(url, headers=headers or {})
    target.parent.mkdir(parents=True, exist_ok=True)
    with urlopen(request, timeout=90) as response, target.open('wb') as output:
        total = 0
        while chunk := response.read(1024 * 1024):
            total += len(chunk)
            if total > LIMIT:
                raise ValueError('repository download exceeds byte limit')
            output.write(chunk)


def extract(archive, target):
    total = 0
    with tarfile.open(archive, 'r:gz') as tar:
        for member in tar:
            name = member.name.removeprefix('./')
            if not name or name == '.':
                continue
            path = relative(name)
            if path.parts[0] not in ('dists', 'pool', 'nl2sh-repo.gpg'):
                raise ValueError(f'unexpected snapshot path: {name}')
            if member.isdir():
                continue
            if not member.isfile():
                raise ValueError('snapshot contains link or special file')
            total += member.size
            if total > LIMIT:
                raise ValueError('expanded repository exceeds byte limit')
            dest = target / path
            if dest.exists():
                raise ValueError(f'duplicate snapshot path: {name}')
            dest.parent.mkdir(parents=True, exist_ok=True)
            with tar.extractfile(member) as source, dest.open('wb') as output:
                shutil.copyfileobj(source, output)


def checked(path, size, digest):
    if path.stat().st_size != size or hashlib.sha256(path.read_bytes()).hexdigest() != digest:
        raise ValueError(f'APT size/SHA-256 mismatch: {path.name}')


def verify(repo, fingerprint=FINGERPRINT, base=None):
    key = (repo / 'nl2sh-repo.gpg').resolve()
    with tempfile.TemporaryDirectory(prefix='nl2sh-apt-key-') as home:
        keys = subprocess.check_output(['gpg', '--homedir', home, '--batch', '--with-colons', '--show-keys', str(key)], text=True, stderr=subprocess.PIPE)
        primary = []
        awaiting = False
        for line in keys.splitlines():
            if line.startswith('pub:'):
                awaiting = True
            elif line.startswith('fpr:') and awaiting:
                primary.append(line.split(':')[9])
                awaiting = False
        if primary != [fingerprint]:
            raise ValueError('APT public key fingerprint mismatch')
        stable = repo / 'dists/stable'
        verified = Path(home) / 'verified-release'
        subprocess.run(['gpgv', '--homedir', home, '--keyring', str(key), '--output', str(verified), str(stable / 'InRelease')], check=True, capture_output=True)
        subprocess.run(['gpgv', '--homedir', home, '--keyring', str(key), str(stable / 'Release.gpg'), str(stable / 'Release')], check=True, capture_output=True)
        if verified.read_bytes() != (stable / 'Release').read_bytes():
            raise ValueError('InRelease and Release disagree')
    release = (stable / 'Release').read_text()
    if not re.search(r'^Codename: stable$', release, re.M) or not re.search(r'^Architectures: aarch64 arm$', release, re.M):
        raise ValueError('unexpected APT distribution or architectures')
    hashes = {}
    active = False
    for line in release.splitlines():
        if line == 'SHA256:':
            active = True
        elif line and not line.startswith(' '):
            active = False
        elif active:
            digest, size, name = line.split()
            relative(name)
            if not re.fullmatch('[0-9a-f]{64}', digest):
                raise ValueError('invalid release digest')
            hashes[name] = (int(size), digest)
    packages = {}
    for arch in ('aarch64', 'arm'):
        for suffix in ('Packages', 'Packages.gz'):
            name = f'main/binary-{arch}/{suffix}'
            size, digest = hashes[name]
            dest = stable / name
            if base:
                fetch(base.rstrip('/') + '/dists/stable/' + quote(name), dest)
            checked(dest, size, digest)
        index = stable / f'main/binary-{arch}/Packages'
        compressed = index.with_name('Packages.gz')
        with gzip.open(compressed, 'rb') as source:
            if source.read(LIMIT + 1) != index.read_bytes():
                raise ValueError('compressed Packages disagrees')
        count = 0
        for paragraph in index.read_text().strip().split('\n\n'):
            fields = dict(line.split(': ', 1) for line in paragraph.splitlines() if ': ' in line and not line.startswith(' '))
            if not fields:
                continue
            if fields.get('Package') != 'nl2sh' or fields.get('Architecture') != arch:
                raise ValueError('unexpected APT package or architecture')
            name = fields['Filename']
            if relative(name).parts[:4] != ('pool', 'main', 'n', 'nl2sh') or not name.endswith('.deb'):
                raise ValueError('unexpected package location')
            packages[name] = (int(fields['Size']), fields['SHA256'])
            count += 1
        if not count:
            raise ValueError(f'empty {arch} package index')
    for name, (size, digest) in packages.items():
        dest = repo / name
        if base:
            fetch(base.rstrip('/') + '/' + quote(name), dest)
        checked(dest, size, digest)
    total = sum(p.stat().st_size for p in repo.rglob('*') if p.is_file())
    if total > LIMIT:
        raise ValueError('repository exceeds total size limit')


def latest_snapshot(destination):
    token = os.environ.get('GH_TOKEN', '')
    headers = {'Accept': 'application/vnd.github+json', 'User-Agent': 'nl2sh-docs'}
    if token:
        headers['Authorization'] = 'Bearer ' + token
    request = Request('https://api.github.com/repos/nl2sh/nl2sh/releases/latest', headers=headers)
    with urlopen(request, timeout=90) as response:
        release = json.load(response)
    assets = [a for a in release['assets'] if a['name'] == 'termux-apt-repository.tar.gz']
    if assets:
        fetch(assets[0]['browser_download_url'], destination)
        return True
    return False


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('site', type=Path)
    parser.add_argument('--archive', type=Path)
    args = parser.parse_args()
    if not (args.site / 'index.html').is_file() or not (args.site / 'en/index.html').is_file():
        parser.error('build the bilingual documentation site first')
    with tempfile.TemporaryDirectory(prefix='nl2sh-apt-') as tmp:
        root = Path(tmp)
        repo = root / 'repo'
        repo.mkdir()
        archive = args.archive or root / 'snapshot.tar.gz'
        if args.archive or latest_snapshot(archive):
            extract(archive, repo)
            verify(repo)
        else:
            print('Latest stable release predates snapshots; verifying existing Pages APT.')
            for name in ('nl2sh-repo.gpg', 'dists/stable/InRelease', 'dists/stable/Release', 'dists/stable/Release.gpg'):
                fetch(BASE + name, repo / name)
            verify(repo, base=BASE)
        # No merge occurs until every signed index and referenced package passes.
        for name in ('dists', 'pool'):
            if (args.site / name).exists():
                raise ValueError(f'APT path collides with docs: {name}')
            shutil.copytree(repo / name, args.site / name)
        shutil.copyfile(repo / 'nl2sh-repo.gpg', args.site / 'nl2sh-repo.gpg')
    print('Merged signature- and SHA-256-verified APT repository; root URLs preserved.')

if __name__ == '__main__':
    main()
