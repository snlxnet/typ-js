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
- [x] shrink the binary
- [x] switch to the latest typst builds
- [ ] set up CI for this repo
- [ ] auto-download libs
- [ ] make clippy happy
- [ ] setEnv method
- [ ] auto-download fonts
- [ ] html export
- [ ] better error reporting
- [ ] better error handling

## Why not typst.ts?
- I couldn't find the docs or examples for adding binary files in the client-only version

