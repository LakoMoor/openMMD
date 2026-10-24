#!/usr/bin/env python3
"""Download a pinned Miku/WAVEFILE test set and all supplied usage notices locally.

Nothing is uploaded or installed. Models/motions have their own terms, not MIT.
No audio is downloaded. Read the downloaded author notices before publication.
"""
import argparse
import hashlib
import json
from pathlib import Path
import urllib.request
import shutil

COMMIT = 'beab9e845f9e5ae11d648f55b24a0e910b56a85a'
BASE = f'https://raw.githubusercontent.com/mrdoob/three.js/{COMMIT}/examples/models/mmd/'
FILES = ['LICENSE', 'Readme.txt', 'miku/readme.txt', 'miku/readme_miku_v2.txt',
         'vmds/readme.txt', 'vmds/readme_wavefile.txt', 'vmds/readme_wavefile_camera.txt',
         'miku/miku_v2.pmd', 'miku/eyeM2.bmp', 'vmds/wavefile_v2.vmd', 'vmds/wavefile_camera.vmd']

def fetch(output):
    manifest = json.loads((Path(__file__).resolve().parent.parent / 'examples/wavefile-manifest.json').read_text())
    expected = {item['path']:item['sha256'] for item in manifest['files']}
    records = []
    for name in FILES:
        target = output / name
        if target.exists():
            payload = target.read_bytes()
        else:
            request = urllib.request.Request(BASE + name, headers={'User-Agent':'openMMD-reference-fetcher'})
            with urllib.request.urlopen(request, timeout=45) as response:
                payload = response.read(16 * 1024 * 1024 + 1)
            if len(payload) > 16 * 1024 * 1024: raise ValueError('Reference file too large')
        if hashlib.sha256(payload).hexdigest() != expected[name]:
            raise ValueError(f'Checksum mismatch for {name}; reference was not changed')
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(payload)
        records.append(dict(path=name, source=BASE+name, bytes=len(payload), sha256=hashlib.sha256(payload).hexdigest()))
        print(name, len(payload))
    (output/'sources.json').write_text(json.dumps(dict(commit=COMMIT,files=records),indent=2)+'\n')
    # Standard toon maps remain external assets from the user's original MMD archive.
    original = Path(__file__).resolve().parent.parent / 'reference/original/Data'
    if original.is_dir():
        (output/'Data').mkdir(exist_ok=True)
        for source in sorted(original.glob('toon*.bmp')):
            shutil.copyfile(source, output/'Data'/source.name)
    print('Downloaded reference data only. Read Readme.txt and the individual author notices.')

if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,default=Path('reference/external/wavefile'))
    args=parser.parse_args()
    fetch(args.output)
