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
    digest = hashlib.sha256()
    count = 0
    with fetch(release_base + 'SpaceHunter.exe') as response:
        first = response.read(65536)
        if not first.startswith(b'MZ'):
            raise SystemExit('The download is not a Windows executable')
        digest.update(first)
        count += len(first)
        for chunk in iter(lambda: response.read(65536), b''):
            digest.update(chunk)
            count += len(chunk)
    with fetch(release_base + 'SpaceHunter.exe.sha256') as response:
        checksum = response.read().decode('utf-8-sig').split()[0]
    actual = digest.hexdigest()
    if actual != info.get('exe_sha256') or actual != checksum:
        raise SystemExit('Public executable checksum does not match release metadata')
    print(f"Verified Space Hunter {info['version']} | {info['commit']} | {count} bytes | SHA-256 {actual}")


if __name__ == '__main__':
    main()
