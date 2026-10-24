#!/usr/bin/env python3
"""Package the native release binary; no reference models or proprietary assets."""
import hashlib
from pathlib import Path
import subprocess
import zipfile

root = Path(__file__).resolve().parent.parent
host = next(line.split(': ', 1)[1] for line in subprocess.check_output(['rustc', '-vV'], text=True).splitlines() if line.startswith('host: '))
executable = 'openmmd.exe' if 'windows' in host else 'openmmd'
binary = root / 'target' / 'release' / executable
if not binary.is_file(): raise SystemExit(f'Missing release executable: {binary}')
name = f'openmmd-{host}'
output = root / 'dist'
output.mkdir(exist_ok=True)
archive = output / f'{name}.zip'
with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as bundle:
    for source, target in [(binary, executable), (root/'LICENSE', 'LICENSE'),
                           (root/'docs'/'release-notes.md', 'README.md'), (root/'docs'/'release-notes.md', 'RELEASE-NOTES.md'),
                           (root/'docs'/'parity.md', 'PARITY.md')]:
        bundle.write(source, f'{name}/{target}')
    for document in sorted((root / 'docs').glob('*.md')):
        bundle.write(document, f'{name}/docs/{document.name}')
checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
(output / f'{name}.sha256').write_text(f'{checksum}  {archive.name}\n')
print(f'{archive.name}: {checksum}')
