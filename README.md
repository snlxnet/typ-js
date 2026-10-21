# typ-js

(work in progress) JS wrapper for typst

## Usage

building: `wasm-pack build --target web --debug`

```js
import init, { TypJs } from "./pkg/typ_js.js";

await init()
const typ = TypJs.new()

const res = await fetch("/jetbrains mono.ttf")
typ.font(new Uint8Array(await res.arrayBuffer()))

typ.add("main.typ", new TextEncoder().encode(input.value))
typ.add("dir/image.png", new Uint8Array(file.arrayBuffer())) // file is a File
await typ.deps("main.typ") // download `@preview/` packages
const { svg, err } = await typ.svg("main.typ")
const { pdf, err } = await typ.pdf("main.typ")

typ.reset()
```

## Goals
- simple API
- wide browser support

I've stopped touching this months ago because I found typst-wasm.
Restarting now because typst-wasm doesn't work in Safari if the server is HTTP.
