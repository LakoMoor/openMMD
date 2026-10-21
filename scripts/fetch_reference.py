#!/usr/bin/env python3
"""Download the user-specified MMD 9.32 distribution for local compatibility tests."""
from pathlib import Path, PurePosixPath
import hashlib
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1] / 'reference'
URL = 'https://learnmmd.com/MikuMikuDanceE_v932x64.zip'
SHA256 = '92c043f3e136f474ab407c198aaaec3e3f81fb02983da3802f169e9f78e1324a'
ROOT.mkdir(exist_ok=True)
archive = ROOT / 'MikuMikuDanceE_v932x64.zip'
if not archive.exists():
    with urllib.request.urlopen(URL, timeout=120) as source:
        data = source.read(64 * 1024 * 1024 + 1)
    if len(data) > 64 * 1024 * 1024:
        raise RuntimeError('Archive exceeds expected download size')
    if hashlib.sha256(data).hexdigest() != SHA256:
        raise RuntimeError('Distribution changed: SHA-256 differs from analyzed archive')
    archive.write_bytes(data)
if hashlib.sha256(archive.read_bytes()).hexdigest() != SHA256:
    raise RuntimeError('Reference archive hash mismatch')
output = ROOT / 'original'
with zipfile.ZipFile(archive) as bundle:
    for member in bundle.infolist():
        name = member.filename
        if not member.flag_bits & 0x800:
            name = name.encode('cp437').decode('cp932', errors='replace')
        relative = PurePosixPath(name.replace('\\', '/'))
        if relative.is_absolute() or '..' in relative.parts:
            raise RuntimeError(f'Unsafe archive path: {name}')
        target = output.joinpath(*relative.parts)
        if member.is_dir():
            target.mkdir(parents=True, exist_ok=True)
        else:
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(bundle.read(member))
print(f'Reference files: {output}')
