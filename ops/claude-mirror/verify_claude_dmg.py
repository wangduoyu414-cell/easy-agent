#!/usr/bin/env python3
import hashlib
import os
import plistlib
import stat
import struct
import subprocess
import sys
import tempfile
from pathlib import Path, PurePosixPath

MAX_ARTIFACT_BYTES = 2 * 1024 * 1024 * 1024
MAX_LISTING_BYTES = 64 * 1024 * 1024
MAX_PLIST_BYTES = 4 * 1024 * 1024
EXPECTED_BUNDLE_ID = 'com.anthropic.claudefordesktop'
EXPECTED_MINIMUM_MACOS_VERSION = '12.0'


def numeric_version(value: str) -> list[int]:
    parts = value.split('.')
    if not 3 <= len(parts) <= 4 or any(not part.isdigit() for part in parts):
        raise ValueError('version is not a three- or four-part numeric version')
    return [int(part) for part in parts]


def versions_equal(left: str, right: str) -> bool:
    left_parts = numeric_version(left)
    right_parts = numeric_version(right)
    width = max(len(left_parts), len(right_parts))
    return left_parts + [0] * (width - len(left_parts)) == right_parts + [0] * (
        width - len(right_parts)
    )


def safe_archive_path(value: str) -> bool:
    path = PurePosixPath(value)
    return bool(value) and not path.is_absolute() and '..' not in path.parts


def archive_paths(path: str) -> list[str]:
    result = subprocess.run(
        ['/usr/bin/7z', 'l', '-slt', path],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=300,
    )
    if result.returncode != 0:
        raise ValueError('7-Zip could not parse the DMG')
    if len(result.stdout) > MAX_LISTING_BYTES:
        raise ValueError('DMG listing exceeds the safety bound')
    listing = result.stdout.decode('utf-8', errors='strict')
    return [line[7:] for line in listing.splitlines() if line.startswith('Path = ')]


def exactly_one(paths: list[str], suffix: str) -> str:
    matches = [value for value in paths if value.endswith(suffix) and safe_archive_path(value)]
    if len(matches) != 1:
        raise ValueError(f'DMG must contain exactly one {suffix}')
    return matches[0]


def macho_architectures(header: bytes) -> set[str]:
    if len(header) < 8:
        raise ValueError('main executable has a truncated Mach-O header')
    magic = header[:4]
    thin = {
        b'\xcf\xfa\xed\xfe': ('little', header[4:8]),
        b'\xfe\xed\xfa\xcf': ('big', header[4:8]),
    }
    cpu_names = {0x01000007: 'x64', 0x0100000C: 'arm64'}
    if magic in thin:
        byteorder, cpu_bytes = thin[magic]
        cpu = int.from_bytes(cpu_bytes, byteorder)
        if cpu not in cpu_names:
            raise ValueError('main executable has an unsupported Mach-O CPU type')
        return {cpu_names[cpu]}
    fat_formats = {
        b'\xca\xfe\xba\xbe': ('>', 20),
        b'\xca\xfe\xba\xbf': ('>', 32),
        b'\xbe\xba\xfe\xca': ('<', 20),
        b'\xbf\xba\xfe\xca': ('<', 32),
    }
    if magic not in fat_formats:
        raise ValueError('main executable is not a supported 64-bit Mach-O')
    endian, entry_size = fat_formats[magic]
    count = struct.unpack(f'{endian}I', header[4:8])[0]
    if count == 0 or count > 16:
        raise ValueError('main executable has an invalid fat slice count')
    required = 8 + count * entry_size
    if len(header) < required:
        raise ValueError('main executable has a truncated fat table')
    architectures = set()
    for index in range(count):
        offset = 8 + index * entry_size
        cpu = struct.unpack(f'{endian}I', header[offset : offset + 4])[0]
        if cpu in cpu_names:
            architectures.add(cpu_names[cpu])
    return architectures


def main() -> int:
    if len(sys.argv) != 3:
        print('usage: verify_claude_dmg.py FILE EXPECTED_VERSION', file=sys.stderr)
        return 2

    artifact_path, expected_version = sys.argv[1:]
    numeric_version(expected_version)
    metadata = os.stat(artifact_path, follow_symlinks=False)
    if not stat.S_ISREG(metadata.st_mode):
        raise ValueError('artifact is not a regular file')
    if metadata.st_size <= 0 or metadata.st_size > MAX_ARTIFACT_BYTES:
        raise ValueError('artifact size is outside the allowed range')

    paths = archive_paths(artifact_path)
    info_entry = exactly_one(paths, '/Claude.app/Contents/Info.plist')
    code_resources_entry = exactly_one(
        paths, '/Claude.app/Contents/_CodeSignature/CodeResources'
    )
    if not code_resources_entry:
        raise ValueError('Claude code resources are absent')

    with tempfile.TemporaryDirectory(prefix='claude-dmg-') as directory:
        extraction = subprocess.run(
            [
                '/usr/bin/7z',
                'x',
                '-y',
                f'-o{directory}',
                artifact_path,
                info_entry,
            ],
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=300,
        )
        if extraction.returncode != 0:
            raise ValueError('could not extract Claude Info.plist from the DMG')
        info_path = Path(directory).joinpath(*PurePosixPath(info_entry).parts)
        if not info_path.is_file() or info_path.stat().st_size > MAX_PLIST_BYTES:
            raise ValueError('Claude Info.plist is absent or too large')
        with info_path.open('rb') as info_file:
            info = plistlib.load(info_file)
        if info.get('CFBundleIdentifier') != EXPECTED_BUNDLE_ID:
            raise ValueError('Claude Bundle ID changed')
        bundle_version = str(
            info.get('CFBundleShortVersionString') or info.get('CFBundleVersion') or ''
        ).strip()
        if not versions_equal(bundle_version, expected_version):
            raise ValueError('Claude bundle version does not match the resolved release')
        if info.get('LSMinimumSystemVersion') != EXPECTED_MINIMUM_MACOS_VERSION:
            raise ValueError('Claude minimum macOS version changed')
        executable = str(info.get('CFBundleExecutable') or '').strip()
        if not executable or any(separator in executable for separator in '/\\:'):
            raise ValueError('Claude executable name is unsafe')
        executable_entry = exactly_one(
            paths, f'/Claude.app/Contents/MacOS/{executable}'
        )
        extraction = subprocess.run(
            [
                '/usr/bin/7z',
                'x',
                '-y',
                f'-o{directory}',
                artifact_path,
                executable_entry,
            ],
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=300,
        )
        if extraction.returncode != 0:
            raise ValueError('could not extract the Claude executable from the DMG')
        executable_path = Path(directory).joinpath(*PurePosixPath(executable_entry).parts)
        with executable_path.open('rb') as executable_file:
            header = executable_file.read(8 + 16 * 32)
        if macho_architectures(header) != {'x64', 'arm64'}:
            raise ValueError('Claude DMG is not the expected Intel/Apple Silicon Universal build')

    digest = hashlib.sha256()
    with open(artifact_path, 'rb') as artifact_file:
        for chunk in iter(lambda: artifact_file.read(1024 * 1024), b''):
            digest.update(chunk)
    print(f'{metadata.st_size}\t{digest.hexdigest()}\t{bundle_version}')
    return 0


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (
        OSError,
        UnicodeError,
        ValueError,
        plistlib.InvalidFileException,
        subprocess.TimeoutExpired,
    ) as error:
        print(f'Claude DMG verification failed: {error}', file=sys.stderr)
        raise SystemExit(1)
