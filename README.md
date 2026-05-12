# typ-js

> A simple JS wrapper for the [typst](https://github.com/typst/typst) compiler.

## Building
```bash
wasm-pack build --dev --target web
```

## Plans
- [x] render to SVG
- [x] render to PDF
- [x] add js `File`s
- [ ] auto-download libs
- [ ] auto-download fonts
- [ ] html export
- [ ] better error reporting
- [ ] better error handling
- [ ] set up CI for this repo, version matching with typst

## Why not typst.ts?
- I couldn't find the docs or examples for adding binary files in the client-only version

