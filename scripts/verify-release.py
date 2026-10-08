#!/usr/bin/env python3
"""Verify that the public Windows download was built from the expected commit."""
import argparse
import hashlib
import json
import urllib.request

BASE = 'https://github.com/quisen/SpaceHunter/releases/latest/download/'


def fetch(url):
    request = urllib.request.Request(url, headers={'User-Agent': 'SpaceHunter-release-check'})
    return urllib.request.urlopen(request, timeout=120)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--commit', required=True, help='Full source commit SHA')
    args = parser.parse_args()
    with fetch(BASE + 'build-info.json') as response:
        info = json.load(response)
    if info.get('commit') != args.commit:
        raise SystemExit(f"Windows download is stale: expected {args.commit}, found {info.get('commit')}")
    # Pin remaining downloads to this immutable release, even if latest moves.
    release_base = f'https://github.com/quisen/SpaceHunter/releases/download/build-{args.commit}/'
    # Older releases only list the Windows .exe.
    assets = info.get('assets') or {'SpaceHunter.exe': {'sha256': info.get('exe_sha256')}}
    magic = {'.exe': b'MZ', '.gz': b'\x1f\x8b'}
    for name, meta in assets.items():
        digest = hashlib.sha256()
        count = 0
        with fetch(release_base + name) as response:
            first = response.read(65536)
            want = next((m for ext, m in magic.items() if name.endswith(ext)), b'')
            if not first.startswith(want):
                raise SystemExit(f'{name} is not the expected file type')
            digest.update(first)
            count += len(first)
            for chunk in iter(lambda: response.read(65536), b''):
                digest.update(chunk)
                count += len(chunk)
        with fetch(release_base + name + '.sha256') as response:
            checksum = response.read().decode('utf-8-sig').split()[0]
        actual = digest.hexdigest()
        if actual != meta.get('sha256') or actual != checksum:
            raise SystemExit(f'{name}: public checksum does not match release metadata')
        # /latest/ must serve this same build
        with fetch(BASE + name + '.sha256') as response:
            if response.read().decode('utf-8-sig').split()[0] != actual:
                raise SystemExit(f'{name}: /releases/latest/ still serves another build')
        print(f"Verified Space Hunter {info['version']} | {info['commit'][:7]} | {name} | {count} bytes | SHA-256 {actual}")

if __name__ == '__main__':
    main()
