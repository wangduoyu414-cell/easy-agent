#!/usr/bin/env python3
import hashlib
import os
import stat
import struct
import subprocess
import sys

MIN_ARTIFACT_BYTES = 1024 * 1024
MAX_ARTIFACT_BYTES = 128 * 1024 * 1024
PE_MACHINES = {'x64': 0x8664, 'arm64': 0xAA64}


def numeric_version(value: str) -> None:
    parts = value.split('.')
    if not 3 <= len(parts) <= 4 or any(not part.isdigit() for part in parts):
        raise ValueError('version is not a three- or four-part numeric version')


def verify_pe_contract(data: bytes, expected_architecture: str) -> None:
    if len(data) < 0x100 or data[:2] != b'MZ':
        raise ValueError('file is not a PE executable')
    pe_offset = struct.unpack_from('<I', data, 0x3C)[0]
    if pe_offset + 24 > len(data) or data[pe_offset : pe_offset + 4] != b'PE\0\0':
        raise ValueError('PE header is absent or truncated')
    machine = struct.unpack_from('<H', data, pe_offset + 4)[0]
    if machine != PE_MACHINES[expected_architecture]:
        raise ValueError(f'PE machine 0x{machine:04X} does not match {expected_architecture}')
    optional_size = struct.unpack_from('<H', data, pe_offset + 20)[0]
    optional_offset = pe_offset + 24
    if optional_offset + optional_size > len(data) or optional_size < 152:
        raise ValueError('PE optional header is absent or truncated')
    if struct.unpack_from('<H', data, optional_offset)[0] != 0x20B:
        raise ValueError('Claude Setup is not a PE32+ executable')
    certificate_offset, certificate_size = struct.unpack_from(
        '<II', data, optional_offset + 112 + 8 * 4
    )
    if (
        certificate_offset == 0
        or certificate_size < 16
        or certificate_offset + certificate_size > len(data)
    ):
        raise ValueError('Authenticode certificate table is absent or invalid')
    certificate_length, revision, certificate_type = struct.unpack_from(
        '<IHH', data, certificate_offset
    )
    if (
        certificate_length < 16
        or certificate_length > certificate_size
        or revision != 0x0200
        or certificate_type != 0x0002
    ):
        raise ValueError('Authenticode WIN_CERTIFICATE header changed')


def verify_version_resources(data: bytes) -> None:
    required = ('Anthropic, PBC', 'Claude Setup', 'ProductName', 'Claude')
    for value in required:
        if value.encode('utf-16le') not in data:
            raise ValueError(f'expected version resource is absent: {value}')


def verify_authenticode(path: str) -> None:
    verifier = '/usr/bin/osslsigncode'
    if not os.path.isfile(verifier):
        raise ValueError('osslsigncode is not installed')
    result = subprocess.run(
        [verifier, 'verify', '-in', path],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=120,
    )
    if result.returncode != 0:
        detail = (result.stdout + result.stderr).decode('utf-8', errors='replace')
        detail = ' '.join(detail.split())[:600]
        raise ValueError(f'Authenticode verification failed: {detail}')


def main() -> int:
    if len(sys.argv) != 4:
        print(
            'usage: verify_claude_setup.py FILE EXPECTED_VERSION EXPECTED_ARCHITECTURE',
            file=sys.stderr,
        )
        return 2

    path, expected_version, expected_architecture = sys.argv[1:]
    numeric_version(expected_version)
    if expected_architecture not in PE_MACHINES:
        raise ValueError('expected architecture must be x64 or arm64')
    file_stat = os.stat(path, follow_symlinks=False)
    if not stat.S_ISREG(file_stat.st_mode):
        raise ValueError('artifact is not a regular file')
    if not MIN_ARTIFACT_BYTES <= file_stat.st_size <= MAX_ARTIFACT_BYTES:
        raise ValueError('artifact size is outside the Claude Setup safety bound')

    with open(path, 'rb') as artifact:
        data = artifact.read(MAX_ARTIFACT_BYTES + 1)
    if len(data) != file_stat.st_size:
        raise ValueError('artifact changed while being read')
    verify_pe_contract(data, expected_architecture)
    verify_version_resources(data)
    verify_authenticode(path)

    digest = hashlib.sha256(data).hexdigest()
    print(f'{file_stat.st_size}\t{digest}\t{expected_version}')
    return 0


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (OSError, ValueError, struct.error, subprocess.SubprocessError) as error:
        print(f'Claude Setup verification failed: {error}', file=sys.stderr)
        raise SystemExit(1)
