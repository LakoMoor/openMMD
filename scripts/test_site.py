#!/usr/bin/env python3
"""Check static localization coverage, supported locales and local links."""
from html import unescape
from html.parser import HTMLParser
import json
from pathlib import Path
import re
import unittest
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parent.parent / 'site'

class Page(HTMLParser):
    def __init__(self, text):
        super().__init__(convert_charrefs=True)
        self.texts = []
        self.attrs = []
        self.feed(text)
    def handle_data(self, text):
        if text.strip(): self.texts.append(text.strip())
    def handle_starttag(self, tag, attrs):
        self.attrs.append((tag, dict(attrs)))

class SiteTests(unittest.TestCase):
    def test_locales_are_complete_and_links_exist(self):
        source = Page((ROOT / 'index.html').read_text())
        unchanged = {'m', 'open', 'MMD', 'EN', '日本語', '简体中文', '☰', '↗', '↓', '↘', '×', 'GitHub ↗',
                     'macOS', 'Windows', 'Linux', 'WebAssembly', 'WASM / WebGPU', 'Rust / wgpu',
                     'PMD', 'PMX 2.0 / 2.1', 'VMD / VPD', 'PMM v1 / v2', 'WebGPU / WASM',
                     'macOS / Windows / Linux', 'RUST + WGPU',
                     'git clone https://github.com/LakoMoor/openMMD.git\ncd openMMD\ncargo run --release'}
        for locale in ('ja', 'zh-Hans'):
            mapping = json.loads((ROOT / 'locales' / f'{locale}.json').read_text())
            self.assertEqual(set(source.texts) - unchanged - set(mapping), set(), locale)
            page = Page((ROOT / locale / 'index.html').read_text())
            self.assertTrue(any(tag == 'html' and a.get('lang') == locale for tag, a in page.attrs))
            self.assertFalse(any(re.search('[А-Яа-яЁё]', text) for text in page.texts))
        for relative in ('index.html', 'ja/index.html', 'zh-Hans/index.html', 'app/index.html'):
            file = ROOT / relative
            page = Page(file.read_text())
            for tag, attrs in page.attrs:
                for key in ('href', 'src'):
                    value = attrs.get(key, '')
                    parsed = urlsplit(value)
                    if parsed.scheme or not parsed.path: continue
                    self.assertTrue((file.parent / unescape(parsed.path)).exists(), f'{relative}: {value}')
            if relative != 'app/index.html':
                choices = [attrs['hreflang'] for _, attrs in page.attrs if 'hreflang' in attrs and 'lang' in attrs]
                self.assertEqual(choices, ['en', 'ja', 'zh-Hans'])

if __name__ == '__main__':
    unittest.main()
