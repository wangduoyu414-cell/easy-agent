#!/usr/bin/env python3
import hashlib
import os
import stat
import sys
import zipfile
import xml.etree.ElementTree as ET

MAX_ARTIFACT_BYTES = 2 * 1024 * 1024 * 1024
MAX_MANIFEST_BYTES = 2 * 1024 * 1024
EXPECTED_PUBLISHER = (
    'CN="Anthropic, PBC", O="Anthropic, PBC", L=San Francisco, '
    'S=California, C=US, SERIALNUMBER=4860621, '
    'OID.2.5.4.15=Private Organization, '
    'OID.1.3.6.1.4.1.311.60.2.1.2=Delaware, '
    'OID.1.3.6.1.4.1.311.60.2.1.3=US'
)


def numeric_version(value: str) -> list[int]:
    parts = value.split('.')
    if not parts or any(not part.isdigit() for part in parts):
        raise ValueError('version is not numeric dot-separated')
    return [int(part) for part in parts]


def versions_equal(left: str, right: str) -> bool:
    left_parts = numeric_version(left)
    right_parts = numeric_version(right)
    width = max(len(left_parts), len(right_parts))
    return left_parts + [0] * (width - len(left_parts)) == right_parts + [0] * (
        width - len(right_parts)
    )


def main() -> int:
    if len(sys.argv) != 4:
        print(
            'usage: verify_claude_msix.py FILE EXPECTED_VERSION EXPECTED_ARCHITECTURE',
            file=sys.stderr,
        )
        return 2

    path, expected_version, expected_architecture = sys.argv[1:]
    if expected_architecture not in {'x64', 'arm64'}:
        raise ValueError('expected architecture must be x64 or arm64')
    file_stat = os.stat(path, follow_symlinks=False)
    if not stat.S_ISREG(file_stat.st_mode):
        raise ValueError('artifact is not a regular file')
    if file_stat.st_size <= 0 or file_stat.st_size > MAX_ARTIFACT_BYTES:
        raise ValueError('artifact size is outside the allowed range')

    digest = hashlib.sha256()
    with open(path, 'rb') as artifact:
        for chunk in iter(lambda: artifact.read(1024 * 1024), b''):
            digest.update(chunk)

    with zipfile.ZipFile(path) as archive:
        bad_member = archive.testzip()
        if bad_member is not None:
            raise ValueError(f'corrupt ZIP member: {bad_member}')
        manifest = archive.read('AppxManifest.xml')
    if len(manifest) > MAX_MANIFEST_BYTES:
        raise ValueError('AppxManifest.xml exceeds 2 MiB')

    root = ET.fromstring(manifest)
    identity = next(
        (node for node in root.iter() if node.tag.rsplit('}', 1)[-1] == 'Identity'),
        None,
    )
    if identity is None:
        raise ValueError('AppxManifest.xml has no Identity')

    name = identity.attrib.get('Name')
    publisher = identity.attrib.get('Publisher')
    package_version = identity.attrib.get('Version')
    architecture = identity.attrib.get('ProcessorArchitecture', '').lower()
    if name != 'Claude':
        raise ValueError(f'unexpected package identity: {name!r}')
    if publisher != EXPECTED_PUBLISHER:
        raise ValueError('unexpected package publisher')
    if architecture != expected_architecture:
        raise ValueError(f'unexpected package architecture: {architecture!r}')
    if package_version is None or not versions_equal(package_version, expected_version):
        raise ValueError(
            f'package version {package_version!r} does not match {expected_version!r}'
        )

    print(f'{file_stat.st_size}\t{digest.hexdigest()}\t{package_version}')
    return 0


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (OSError, ValueError, zipfile.BadZipFile, ET.ParseError) as error:
        print(f'Claude MSIX verification failed: {error}', file=sys.stderr)
        raise SystemExit(1)
