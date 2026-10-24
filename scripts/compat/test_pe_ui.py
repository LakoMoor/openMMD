import struct
import unittest
from inspect_pe_ui import dialog

def text(value): return value.encode('utf-16le') + b'\0\0'

def fixture(extended):
    if extended:
        data = struct.pack('<HHIIIHhhhh',1,0xffff,0,0,0x40,1,0,0,80,40)
    else:
        data = struct.pack('<IIHhhhh',0x40,0,1,0,0,80,40)
    data += b'\0\0\0\0' + text('camera') + struct.pack('<H',9)
    if extended: data += struct.pack('<HBB',400,0,1)
    data += text('Arial')
    data += b'\0' * ((-len(data)) % 4)
    if extended: data += struct.pack('<IIIhhhhI',0,0,0x50000000,2,3,20,10,456)
    else: data += struct.pack('<IIhhhhH',0x50000000,0,2,3,20,10,456)
    return data + struct.pack('<HH',0xffff,0x80) + text('OK') + b'\0\0'

class DialogTests(unittest.TestCase):
    def test_standard_and_extended_fields(self):
        for extended in (False,True):
            result=dialog(fixture(extended))
            self.assertEqual(result['extended'],extended)
            self.assertEqual(result['rect'],[0,0,80,40])
            self.assertEqual(result['title'],'camera')
            self.assertEqual(result['font']['face'],'Arial')
            self.assertEqual(result['controls'][0]['id'],456)
            self.assertEqual(result['controls'][0]['control_class'],{'ordinal':0x80})
            self.assertEqual(result['controls'][0]['text'],'OK')
            self.assertEqual(result['controls'][0]['rect'],[2,3,20,10])
    def test_truncated_records_fail(self):
        for extended in (False,True):
            data=fixture(extended)
            for end in range(len(data)):
                with self.assertRaises((ValueError,struct.error,UnicodeDecodeError)):
                    dialog(data[:end])

if __name__=='__main__': unittest.main()
