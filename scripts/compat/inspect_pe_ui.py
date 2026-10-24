#!/usr/bin/env python3
"""Inventory Win32 dialog templates without executing or modifying a PE file.

This reports declared dialog units, not observed screen pixels or runtime state.
Use only on a locally supplied reference binary; no binary resources are bundled.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct

class Reader:
    def __init__(self, data, pos=0): self.data, self.pos = data, pos
    def take(self, size):
        end = self.pos + size
        if size < 0 or end > len(self.data): raise ValueError('Truncated structure')
        value = self.data[self.pos:end]; self.pos = end; return value
    def read(self, fmt): return struct.unpack('<' + fmt, self.take(struct.calcsize('<' + fmt)))
    def align(self): self.take((-self.pos) % 4)
    def field(self):
        start = self.pos
        value, = self.read('H')
        if value == 0xffff: return {'ordinal': self.read('H')[0]}
        if value == 0: return ''
        while self.read('H')[0]: pass
        return self.data[start:self.pos-2].decode('utf-16le')


def dialog(data):
    r = Reader(data)
    extended = data[:4] == b'\x01\x00\xff\xff'
    if extended:
        version, signature, help_id, exstyle, style, count, x, y, w, h = r.read('HHIIIHhhhh')
    else:
        style, exstyle, count, x, y, w, h = r.read('IIHhhhh')
    menu, window_class, title = r.field(), r.field(), r.field()
    font = None
    if style & 0x40:
        size, = r.read('H')
        weight, italic, charset = r.read('HBB') if extended else (None, None, None)
        font = dict(size=size, weight=weight, italic=italic, charset=charset, face=r.field())
    controls = []
    for _ in range(count):
        r.align()
        if extended:
            help_id, exstyle, style, x1, y1, w1, h1, identifier = r.read('IIIhhhhI')
        else:
            style, exstyle, x1, y1, w1, h1, identifier = r.read('IIhhhhH')
        control_class, text = r.field(), r.field()
        extra_size, = r.read('H')
        # Creation data size includes its own size WORD for a nonzero block.
        if extra_size: r.take(max(0, extra_size - 2))
        controls.append(dict(id=identifier, rect=[x1,y1,w1,h1], control_class=control_class,
                             text=text, style=hex(style), extended_style=hex(exstyle)))
    return dict(extended=extended, rect=[x,y,w,h], title=title, menu=menu,
                window_class=window_class, font=font, controls=controls)


def inspect(path):
    data = path.read_bytes()
    if data[:2] != b'MZ': raise ValueError('Not a PE file')
    pe = struct.unpack_from('<I', data, 0x3c)[0]
    if data[pe:pe+4] != b'PE\0\0': raise ValueError('Invalid PE header')
    count = struct.unpack_from('<H', data, pe+6)[0]
    opt_size = struct.unpack_from('<H', data, pe+20)[0]
    opt = pe+24
    magic = struct.unpack_from('<H', data, opt)[0]
    if magic not in (0x10b, 0x20b): raise ValueError('Unsupported optional header')
    directory = opt + (112 if magic == 0x20b else 96)
    resource_rva, resource_size = struct.unpack_from('<II', data, directory+16)
    sections=[]
    for i in range(count):
        start = opt+opt_size+i*40
        size, rva, raw_size, raw = struct.unpack_from('<IIII', data, start+8)
        sections.append((rva, raw_size, raw))
    def offset(rva, size=1):
        for base, length, raw in sections:
            if base <= rva and rva+size <= base+length and raw+(rva-base)+size <= len(data):
                return raw+(rva-base)
        raise ValueError('Unmapped resource RVA')
    root = offset(resource_rva, resource_size)
    resource = data[root:root+resource_size]
    def name(value):
        if not value & 0x80000000: return value
        r=Reader(resource,value&0x7fffffff);length,=r.read('H')
        return r.take(length*2).decode('utf-16le')
    entries=[]
    def walk(pos, parts):
        if len(parts)>3: raise ValueError('Invalid resource depth')
        r=Reader(resource,pos);r.take(12);named,number=r.read('HH')
        for _ in range(named+number):
            key,target=r.read('II');current=parts+[name(key)]
            if target&0x80000000: walk(target&0x7fffffff,current)
            else:
                item=Reader(resource,target);rva,size,codepage,reserved=item.read('IIII')
                start=offset(rva,size)
                entries.append((current,data[start:start+size]))
    walk(0,[])
    dialogs=[]
    for parts,blob in entries:
        if parts[0] == 5:
            dialogs.append(dict(resource_path=parts, sha256=hashlib.sha256(blob).hexdigest(), **dialog(blob)))
    return dict(binary_sha256=hashlib.sha256(data).hexdigest(),
                authority='shipped-artifact', runtime_observed=False, coordinate_unit='dialog-unit',
                resource_count=len(entries), dialogs=dialogs)

if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary',type=Path)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    result=inspect(args.binary)
    args.output.write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
    print(json.dumps(dict(binary_sha256=result['binary_sha256'], resources=result['resource_count'],
                         dialogs=len(result['dialogs']), controls=sum(len(d['controls']) for d in result['dialogs']))))
