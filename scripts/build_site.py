#!/usr/bin/env python3
"""Build complete static Japanese and Simplified Chinese pages from the English page.

No runtime translation or third-party services. Keep localized pages tracked so
GitHub Pages and no-JavaScript visitors receive the same complete content.
"""
from html import escape, unescape
from html.parser import HTMLParser
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parent.parent / 'site'
LANGUAGES = ('en', 'ja', 'zh-Hans')


def language_links(current, prefix):
    names = ('EN', '日本語', '简体中文')
    links = []
    for code, name in zip(LANGUAGES, names):
        href = prefix if code == 'en' else prefix + code + '/'
        active = ' aria-current="page"' if code == current else ''
        links.append(f'<a href="{href or "./"}" lang="{code}" hreflang="{code}"{active}>{name}</a>')
    label = {'en': 'Language', 'ja': '言語', 'zh-Hans': '语言'}[current]
    return f'<div class="languages" aria-label="{label}">' + ''.join(links) + '</div>'


def build():
    source = (ROOT / 'index.html').read_text()
    source = re.sub(r'<!-- language-links -->.*?<!-- /language-links -->',
                    '<!-- language-links -->' + language_links('en', './') + '<!-- /language-links -->', source, flags=re.S)
    source = re.sub(r'<!-- alternate-links -->.*?<!-- /alternate-links -->\s*', '', source, flags=re.S)
    alternates = ''.join(f'<link rel="alternate" hreflang="{lang}" href="https://lakomoor.github.io/openMMD/{"" if lang == "en" else lang + "/"}">' for lang in LANGUAGES)
    source = source.replace('</head>', '<!-- alternate-links -->' + alternates + '<!-- /alternate-links -->\n</head>')
    (ROOT / 'index.html').write_text(source)
    for language in LANGUAGES[1:]:
        translations = json.loads((ROOT / 'locales' / f'{language}.json').read_text())
        def translate_text(match):
            raw = match.group(0)
            text = unescape(raw.strip())
            if text in translations:
                return raw.replace(raw.strip(), escape(translations[text], quote=False))
            return raw
        # Tags and their attribute values are handled separately from text nodes.
        page = ''.join(part if part.startswith('<') else translate_text(re.match(r'[\s\S]*', part))
                       for part in re.split(r'(<[^>]*>)', source))
        def translate_attr(match):
            return match[1] + escape(translations.get(unescape(match[2]), unescape(match[2])), quote=True) + '"'
        page = re.sub(r'((?:aria-label|alt|data-caption|content)=")([^"]*)"', translate_attr, page)
        page = page.replace('<html lang="en">', f'<html lang="{language}">')
        page = re.sub(r'<!-- language-links -->.*?<!-- /language-links -->',
                      '<!-- language-links -->' + language_links(language, '../') + '<!-- /language-links -->', page, flags=re.S)
        page = page.replace('href="style.css"', 'href="../style.css"').replace('src="app.js"', 'src="../app.js"')
        page = page.replace('href="app/"', f'href="../app/?lang={language}"')
        page = page.replace('href="assets/', 'href="../assets/').replace('src="assets/', 'src="../assets/')
        # The translated mobile table uses a real label from HTML instead of CSS-generated English.
        page = page.replace('data-limit-label="Still ahead"', f'data-limit-label="{translations["Still ahead"]}"')
        target = ROOT / language
        target.mkdir(exist_ok=True)
        (target / 'index.html').write_text(page)


if __name__ == '__main__':
    build()
