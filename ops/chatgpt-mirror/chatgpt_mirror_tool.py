#!/usr/bin/env python3
import argparse
import base64
import hashlib
import os
import plistlib
import re
import stat
import struct
import subprocess
import sys
import tempfile
import zipfile
from pathlib import PurePosixPath
from urllib.parse import urlparse
from xml.etree import ElementTree

SPARKLE_PUBLIC_KEY = "mNfr1v9t63BfgDtlw4C8lRvSY6uMggIXABDOCi3tS6k="
MAX_ARTIFACT_BYTES = 2 * 1024 * 1024 * 1024
MAX_EXPANDED_BYTES = 12 * 1024 * 1024 * 1024
MAX_ARCHIVE_ENTRIES = 200_000
EXPECTED_BUNDLE_ID = "com.openai.codex"


def local_name(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def direct_child_text(element, name: str):
    for child in element:
        if local_name(child.tag) == name and child.text:
            value = child.text.strip()
            if value:
                return value
    return None


def decode_signature(value: str) -> bytes:
    try:
        decoded = base64.b64decode(value, validate=True)
    except ValueError as error:
        raise ValueError("Sparkle signature is not valid base64") from error
    if len(decoded) != 64:
        raise ValueError("Sparkle signature is not 64 bytes")
    return decoded


def parse_appcast(path: str, architecture: str):
    if architecture not in {"x64", "arm64"}:
        raise ValueError("unsupported architecture")
    root = ElementTree.parse(path).getroot()
    item = next((node for node in root.iter() if local_name(node.tag) == "item"), None)
    if item is None:
        raise ValueError("appcast has no release item")
    version = direct_child_text(item, "shortVersionString") or direct_child_text(item, "title")
    if version is None or re.fullmatch(r"[0-9]+(?:\.[0-9]+)+", version) is None:
        raise ValueError("appcast version is invalid")
    minimum = direct_child_text(item, "minimumSystemVersion")
    if minimum is None or re.fullmatch(r"[0-9]+(?:\.[0-9]+)+", minimum) is None:
        raise ValueError("appcast minimum macOS version is invalid")
    if architecture == "arm64" and direct_child_text(item, "hardwareRequirements") != "arm64":
        raise ValueError("ARM64 appcast hardware requirement changed")
    enclosure = next((child for child in item if local_name(child.tag) == "enclosure"), None)
    if enclosure is None:
        raise ValueError("appcast has no full-package enclosure")
    expected_file = f"ChatGPT-darwin-{architecture}-{version}.zip"
    expected_url = f"https://persistent.oaistatic.com/codex-app-prod/{expected_file}"
    url = enclosure.attrib.get("url")
    if url != expected_url:
        raise ValueError("appcast artifact URL changed")
    parsed = urlparse(url)
    if parsed.query or parsed.fragment:
        raise ValueError("appcast artifact URL has query or fragment")
    if enclosure.attrib.get("type") != "application/octet-stream":
        raise ValueError("appcast artifact content type changed")
    try:
        size = int(enclosure.attrib["length"])
    except (KeyError, ValueError) as error:
        raise ValueError("appcast artifact size is invalid") from error
    if size <= 0 or size > MAX_ARTIFACT_BYTES:
        raise ValueError("appcast artifact size is outside the safety bound")
    signature = next(
        (value.strip() for key, value in enclosure.attrib.items() if local_name(key) == "edSignature"),
        None,
    )
    if not signature:
        raise ValueError("appcast Sparkle signature is absent")
    decode_signature(signature)
    print(f"{version}\t{size}\t{signature}\t{url}\t{minimum}")


def safe_archive_name(name: str) -> bool:
    path = PurePosixPath(name)
    return bool(name) and not path.is_absolute() and ".." not in path.parts


def macho_architectures(header: bytes) -> set[str]:
    if len(header) < 8:
        raise ValueError("main executable has a truncated Mach-O header")
    magic = header[:4]
    thin = {
        b"\xcf\xfa\xed\xfe": ("little", header[4:8]),
        b"\xfe\xed\xfa\xcf": ("big", header[4:8]),
    }
    cpu_names = {0x01000007: "x64", 0x0100000C: "arm64"}
    if magic in thin:
        byteorder, cpu_bytes = thin[magic]
        cpu = int.from_bytes(cpu_bytes, byteorder)
        if cpu not in cpu_names:
            raise ValueError("main executable has an unsupported Mach-O CPU type")
        return {cpu_names[cpu]}
    fat_formats = {
        b"\xca\xfe\xba\xbe": (">", 20),
        b"\xca\xfe\xba\xbf": (">", 32),
        b"\xbe\xba\xfe\xca": ("<", 20),
        b"\xbf\xba\xfe\xca": ("<", 32),
    }
    if magic not in fat_formats:
        raise ValueError("main executable is not a supported 64-bit Mach-O")
    endian, entry_size = fat_formats[magic]
    count = struct.unpack(f"{endian}I", header[4:8])[0]
    if count == 0 or count > 16:
        raise ValueError("main executable has an invalid fat slice count")
    required = 8 + count * entry_size
    if len(header) < required:
        raise ValueError("main executable has a truncated fat table")
    result = set()
    for index in range(count):
        offset = 8 + index * entry_size
        cpu = struct.unpack(f"{endian}I", header[offset : offset + 4])[0]
        if cpu in cpu_names:
            result.add(cpu_names[cpu])
    if not result:
        raise ValueError("main executable has no supported architecture")
    return result


def verify_zip(
    path: str,
    architecture: str,
    version: str,
    minimum_macos_version: str,
    expected_size: int,
    signature_text: str,
):
    if re.fullmatch(r"[0-9]+(?:\.[0-9]+)+", minimum_macos_version) is None:
        raise ValueError("minimum macOS version is invalid")
    metadata = os.lstat(path)
    if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode):
        raise ValueError("artifact is not a regular file")
    if metadata.st_size != expected_size or metadata.st_size <= 0 or metadata.st_size > MAX_ARTIFACT_BYTES:
        raise ValueError("downloaded artifact size does not match the appcast")
    signature = decode_signature(signature_text)

    with zipfile.ZipFile(path) as archive:
        entries = archive.infolist()
        if len(entries) > MAX_ARCHIVE_ENTRIES:
            raise ValueError("ZIP has too many entries")
        seen = set()
        expanded = 0
        for entry in entries:
            if not safe_archive_name(entry.filename):
                raise ValueError("ZIP contains an unsafe path")
            folded = entry.filename.casefold()
            if folded in seen:
                raise ValueError("ZIP contains a duplicate path")
            seen.add(folded)
            expanded += entry.file_size
            if expanded > MAX_EXPANDED_BYTES:
                raise ValueError("ZIP expanded size exceeds the safety bound")
            if entry.flag_bits & 1:
                raise ValueError("ZIP contains an encrypted entry")
        info_name = "ChatGPT.app/Contents/Info.plist"
        try:
            info_entry = archive.getinfo(info_name)
        except KeyError as error:
            raise ValueError("ZIP has no ChatGPT Info.plist") from error
        if info_entry.file_size <= 0 or info_entry.file_size > 4 * 1024 * 1024:
            raise ValueError("Info.plist is outside the safety bound")
        info = plistlib.loads(archive.read(info_entry))
        if info.get("CFBundleIdentifier") != EXPECTED_BUNDLE_ID:
            raise ValueError("ChatGPT Bundle ID changed")
        bundle_version = str(
            info.get("CFBundleShortVersionString") or info.get("CFBundleVersion") or ""
        ).strip()
        if bundle_version != version:
            raise ValueError("ChatGPT bundle version does not match the appcast")
        bundle_minimum = str(info.get("LSMinimumSystemVersion") or "").strip()
        if bundle_minimum != minimum_macos_version:
            raise ValueError("ChatGPT bundle minimum macOS version does not match the appcast")
        executable = str(info.get("CFBundleExecutable") or "").strip()
        if not executable or any(separator in executable for separator in "/\\:"):
            raise ValueError("ChatGPT executable name is unsafe")
        executable_name = f"ChatGPT.app/Contents/MacOS/{executable}"
        try:
            with archive.open(executable_name) as executable_file:
                header = executable_file.read(8 + 16 * 32)
        except KeyError as error:
            raise ValueError("ZIP has no ChatGPT main executable") from error
        if architecture not in macho_architectures(header):
            raise ValueError("ChatGPT main executable architecture does not match the appcast")

    public_key_der = bytes.fromhex("302a300506032b6570032100") + base64.b64decode(
        SPARKLE_PUBLIC_KEY
    )
    with tempfile.TemporaryDirectory(prefix="chatgpt-sparkle-") as directory:
        public_key_path = os.path.join(directory, "public.der")
        signature_path = os.path.join(directory, "signature.bin")
        with open(public_key_path, "wb") as public_key_file:
            public_key_file.write(public_key_der)
        with open(signature_path, "wb") as signature_file:
            signature_file.write(signature)
        verification = subprocess.run(
            [
                "/usr/bin/openssl",
                "pkeyutl",
                "-verify",
                "-pubin",
                "-keyform",
                "DER",
                "-inkey",
                public_key_path,
                "-rawin",
                "-in",
                path,
                "-sigfile",
                signature_path,
            ],
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=300,
        )
        if verification.returncode != 0:
            raise ValueError("OpenAI Sparkle signature verification failed")

    digest = hashlib.sha256()
    with open(path, "rb") as artifact_file:
        for chunk in iter(lambda: artifact_file.read(1024 * 1024), b""):
            digest.update(chunk)
    print(f"{metadata.st_size}\t{digest.hexdigest()}\t{bundle_version}")


def main() -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    appcast = subparsers.add_parser("parse-appcast")
    appcast.add_argument("path")
    appcast.add_argument("architecture", choices=["x64", "arm64"])
    artifact = subparsers.add_parser("verify-zip")
    artifact.add_argument("path")
    artifact.add_argument("architecture", choices=["x64", "arm64"])
    artifact.add_argument("version")
    artifact.add_argument("minimum_macos_version")
    artifact.add_argument("expected_size", type=int)
    artifact.add_argument("signature")
    arguments = parser.parse_args()
    try:
        if arguments.command == "parse-appcast":
            parse_appcast(arguments.path, arguments.architecture)
        else:
            verify_zip(
                arguments.path,
                arguments.architecture,
                arguments.version,
                arguments.minimum_macos_version,
                arguments.expected_size,
                arguments.signature,
            )
    except (OSError, ValueError, zipfile.BadZipFile, ElementTree.ParseError) as error:
        print(f"ChatGPT mirror verification failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
