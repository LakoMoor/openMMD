# Website languages

The GitHub Pages website supports exactly three locales:

- English: `/openMMD/`
- Japanese: `/openMMD/ja/`
- Simplified Chinese: `/openMMD/zh-Hans/`

Each page is a complete static document, including translated descriptions,
image alternatives, captions, navigation and compatibility limits. Navigation
works without JavaScript. English remains the default; visitors choose their
language explicitly. Product names, file formats and shell commands are kept
unchanged.

`site/index.html` is the English source. `site/locales/ja.json` and
`site/locales/zh-Hans.json` contain the translations. Rebuild and check the
tracked localized HTML pages after editing content:

```sh
python3 scripts/build_site.py
python3 scripts/test_site.py
```

The Pages workflow runs both commands before building the WebAssembly editor.
The tests check text coverage, supported language choices, local assets and
links. The generator is deterministic and uses only Python's standard library.

The browser launch page accepts `?lang=en`, `?lang=ja` or `?lang=zh-Hans`.
Its language selector updates the page without reloading or dropping imported
files. The choice is retained in browser local storage when available. GPU
initialization and loading errors are translated too. The Rust editor's own
controls and technical diagnostics currently remain in English.

The landing pages have been checked at widths of 320, 390, 768 and 1440 CSS
pixels for all three locales. Mobile layout checks emulate a browser viewport;
they do not certify WebGPU support on physical phones.
