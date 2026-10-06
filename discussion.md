# Rust browser components research

**User:** Anonymous  
**Created:** 9/30/2026 22:13:50  
**Updated:** 9/30/2026 23:26:56  
**Exported:** 9/30/2026 23:38:02  
**Link:** [https://chatgpt.com/c/6abd3c45-b7bc-83ee-a37b-1645c31d2d66](https://chatgpt.com/c/6abd3c45-b7bc-83ee-a37b-1645c31d2d66)

## Prompt:

30/09/2026, 22:13:50

If suppose i take the monumental task of creating a browser completely from scratch with everything written in rust. (I may take components of browser that have already been implemented in rust to make my life a little bit easy.. but everything that has not yet been made, we must make it in rust). This browser is not supposed to be a demo or a proof of concept. It is supposed to be end-to-end finished product on par with any other browser like chrome or edge. What is the most exhaustive list of components that i would have to implement to get a completely working browser. Characterize these components in three types: Already written in rust and modular enough to use, Not written in rust but can reliably be ported (with absolute guarantee that simply porting it wont cause any bugs), Need to be written in rust from scratch. Do a deep research.@Deep research

## Response:

30/09/2026, 22:14:02 · gpt-5-6

# Browser Architecture and Key Components

Building a full-featured browser requires implementing many subsystems. At a high level, a browser includes a **UI (window and controls)**, a **browser engine** (the application logic), a **rendering engine** (HTML/CSS parser, DOM, layout, painting), a **JavaScript engine**, networking (HTTP, TLS, DNS, etc.), **storage** (cache, cookies, IndexedDB, etc.), multimedia (audio/video decoding, graphics), **security** (TLS, certificates, sandboxing), and various ancillary features (developer tools, extensions, printing, accessibility, etc.). An example breakdown is in Will T.’s browser components series, which lists UI, engine, renderer, networking, JS engine, UI backend and storage.

Each of these subsystems must be written (or re-used) in Rust. We classify components as **(A) Already in Rust (with suitable crates)**, **(B) Non-Rust but portable to Rust**, or **(C) Must implement anew in Rust**.

## Rendering Engine (HTML/CSS/Graphics)

- **HTML Parser & DOM** – _Already in Rust:_ The Servo project provides _html5ever_, a WHATWG-compliant HTML5 parser in Rust. html5ever is production-grade and used in other Rust projects. It parses HTML into a DOM; servo also provides example DOM models (e.g. the _rcdom_ tree builder). Thus HTML parsing and basic DOM construction is available in Rust.

- **CSS Parsing and Styling** – _Already in Rust:_ Servo has _rust-cssparser_, a Rust implementation of the CSS Syntax Level 3 tokenizer/parser. This crate can tokenize CSS and produce syntax structures (component values). A complete style engine (matching selectors, computing inherited and cascade rules) exists in Servo’s style subsystem (also Rust), though that may require integrating multiple crates. In practice, CSS parsing and style matching can be done with the combination of rust-cssparser and Servo’s style/layout crates.

- **Layout Engine** – _Already in Rust:_ Servo implements layout logic in Rust (its _layout_ and _style_ modules). This handles box generation and formatting. The Servo project is a full Rust browser engine prototype, so its layout code (flexbox, grid, block layout, etc.) is in Rust. A re-usable layout crate may not be standalone, but the code exists in Servo’s repository.

- **Painting / Compositing** – _Already in Rust:_ Servo’s GPU compositor is **WebRender**, a high-performance 2D GPU-based renderer written in Rust. WebRender is used in Firefox and Servo, and is available as a Rust crate. It takes drawing commands (quads, text glyphs, images) and renders via OpenGL/Vulkan. Thus the core painting/compositing layer is already Rust.

- **2D Canvas & Vector Graphics** – _Already in Rust:_ For HTML5 Canvas and SVG rendering, Mozilla’s _Pathfinder_ library (from Servo) is a GPU-accelerated vector graphics renderer in Rust. It implements the Canvas API and can parse/render SVG. Pathfinder can draw paths, shapes, and fonts with GPU and SIMD acceleration. Therefore 2D graphics APIs (Canvas, SVG) can use Pathfinder (Rust).

- **Fonts & Text** – _Already in Rust:_ The _font-kit_ crate provides font handling: it finds system fonts, matches font families, and rasterizes glyphs. Font-kit is pure Rust and can use system backends (CoreText, DirectWrite, FreeType). For advanced text shaping, one might integrate _pathfinder_ or libraries like _fontdue_, but basic font loading/rasterization is covered.

- **Images** – _Already in Rust:_ Common image formats have Rust crates (e.g. _image_, _png_, _jpeg_, _gif_). These crates can decode images into bitmaps for compositing. No need to port C libraries for basic formats.

- **WebGL / WebGPU** – _Partial:_ For WebGL support, one can use Rust OpenGL or WebGPU bindings (e.g. _gl_ or _wgpu_ crates). These are Rust wrappers for GPU APIs, so mostly “already Rust” to use, though one must implement the WebGL API layer. For WebGPU, there is the wgpu Rust library. These are available, but integrating them with the browser and GLSL translation is work.

**Classification:** Almost all core rendering components have mature Rust implementations. html5ever and rust-cssparser handle parsing; WebRender and Pathfinder handle painting; font-kit covers fonts; image decoding crates exist in Rust. The style/layout code from Servo is Rust. In summary, HTML/CSS layout and drawing can largely reuse existing Rust crates. Any missing pieces (e.g. parts of CSS cascade or new CSS features) would need new Rust code, but the foundation is “Already in Rust.”

## JavaScript and WebAssembly

- **JavaScript Engine** – _Not currently complete in Rust:_ Major JS engines (V8, SpiderMonkey, JavaScriptCore) are C++ and not written in Rust, so they are not directly portable. However, there is **Boa**, a pure-Rust JavaScript engine under development. Boa implements much of ECMAScript and is being actively developed. For example, the headless Rust browser _h5i_ uses Boa for JavaScript execution. That shows a path to a Rust JS engine (Boa) is possible. However, Boa is not yet as complete or fast as V8. In practice, one would either embed an existing engine (violating “all Rust”) or invest effort to finish/optimize Boa (a huge task). Given the question’s constraints, the JS engine is a critical component: if strict Rust is required, **writing or completing a JS engine in Rust is needed**. (Category: _Need new Rust implementation_ – though Boa is a starting point.)

- **WebAssembly (Wasm)** – _Already in Rust:_ There are Rust crates for Wasm execution, such as _wasmtime_ (a fast JIT-based Wasm runtime in Rust) and _wasmi_ (an interpreter). Both are pure Rust and can be embedded. A modern browser should run Wasm, so including a Wasm engine like Wasmtime (Rust) is feasible. (No need to port C++ Wasm engines.)

**Classification:** The JS engine is the big gap: no production-grade JS engine in Rust exists yet, so this likely falls under “implement from scratch (or port existing logic)”. Boa (Rust) covers some JS, but not all, so writing new Rust code is needed. WebAssembly support can be handled by existing Rust crates (wasmtime/wasmi), so that part is “already Rust”.

## Networking and Protocols

- **HTTP/HTTPS** – _Already in Rust:_ The Rust ecosystem has HTTP libraries. For HTTP/1.x and HTTP/2, the **hyper** crate provides client and server implementations in Rust. For HTTPS/TLS, **rustls** is a pure-Rust TLS implementation. A browser needs both HTTP stack and TLS. These crates cover HTTP and TLS. For example, reqwest (built on hyper) can be used as an HTTP client.
- **HTTP/3 (QUIC)** – _Already in Rust:_ The _quinn_ crate is an open-source QUIC (HTTP/3) implementation in Rust. If supporting HTTP/3, Quinn can be used.
- **WebSockets, DNS, Other Protocols** – _Already in Rust:_ WebSocket libraries like _tungstenite_ (pure Rust) and DNS libraries like _trust-dns_ (Rust) exist. Any socket programming and async runtime (tokio, async-std) is in Rust.

**Classification:** Networking is well-covered by Rust crates (hyper, rustls, quinn, trust-dns, etc.). These can be used directly. There is no need to port C libraries for core networking; the necessary protocols are implemented in Rust.

## Data Storage and Persistence

A full browser stores state: cookies, caches, history, bookmarks, localStorage/IndexedDB, etc.

- **Cookies, Cache, History** – One can implement these with file storage or embedded databases. A Rust crate like _sled_ (a fast embedded key-value store in Rust) could be used. Alternatively, _sqlite_ (via rusqlite, which uses C but has safe bindings) could handle structured data.
- **localStorage / IndexedDB** – Similarly, these can map to embedded DB storage. For example, IndexedDB can use an SQLite or a custom store. There is no out-of-the-box “IndexedDB” Rust crate; it would need implementing on top of a DB.
- **File System Access** – Using Rust’s standard library, file APIs are available natively.
- **User Settings, Extensions, Profiles** – All of these can be implemented with existing Rust crates for serialization (JSON, YAML) and storage.

**Classification:** Basic data storage can use Rust crates (sled, etc.). No specialized Rust browser storage library exists, but general DB/file crates can be used. Thus, one would “build in Rust” using these tools (likely Category A for using existing crates like sled, but the browser-specific logic must be implemented).

## User Interface and Platform Integration

- **Windowing and Events** – _Already in Rust:_ Cross-platform window/event libraries like _winit_ (Rust) allow creating windows and handling input events. Graphics contexts (OpenGL/Vulkan) can use crates like _glutin_ or _wgpu_. These are Rust or have Rust bindings. For example, a browser can create a window with winit and get GPU surfaces.
- **Native UI Controls** – _Need new / port:_ Traditional browsers have menus, toolbars, dialogs. There is no single “browser UI toolkit” in Rust. One might use a Rust GUI framework (e.g. _iced_, _egui_, _druid_), but these are general-purpose and not browser-tailored. Alternatively, binding to GTK/Qt (C libraries) is possible but violates “pure Rust”. Likely the UI (address bar, bookmarks, etc.) would be implemented in Rust using a GUI crate or custom code.
- **UI Backend (Form Controls, Widgets)** – Web form widgets (select boxes, file pickers, etc.) often rely on OS drawing. In Rust, there’s no universal UI backend API; one would integrate with the OS via GUI crates or draw custom controls. This typically means writing new code or bindings.
- **Accelerated Graphics** – Drawing the web page content is handled by WebRender/Pathfinder (see above). Integrating that with the window (via OpenGL or DirectX/Vulkan) can use existing Rust GPU crates.

**Classification:** Windowing/event loop crates (winit/glutin) are Rust (Category A). The browser chrome (menus, controls) has no dedicated Rust solution, so it “needs to be built” – either with existing Rust GUI libraries or by porting a GUI toolkit (which is very complex). We would mark the UI chrome and form widget layer as **needing to implement in Rust**, perhaps using GUI crates as foundations.

## Multimedia (Audio/Video)

- **Audio Decoding/Playback** – _Already in Rust:_ The _Symphonia_ project is a pure-Rust multimedia library that supports audio decoding for many formats (MP3, AAC, Opus, WAV, Vorbis, etc.) and container demuxing. It can decode audio streams from common web media. For playback, Rust crates like _rodio_ or _cpal_ (audio output) can be used. Thus audio pipeline can be Rust.
- **Video Decoding** – _Not in Rust:_ There is no mature pure-Rust video decoder for codecs like H.264/VP9/etc. Typically browsers rely on ffmpeg or platform decoders (C/C++). Porting video codecs to Rust is a major effort. One could perhaps use _gstreamer-rs_ (bindings to GStreamer C libraries) or port small libraries, but realistically video decoding would require FFI to existing libs or writing huge new Rust code.
- **Media Containers** – Symphonia covers some container formats (MP4, MKV, OGG) for demuxing. For video containers, one might leverage Symphonia’s demuxers where available.
- **Streaming, DRM** – Features like encrypted media (DRM) have no Rust equivalents; would need native/third-party components.

**Classification:** Audio is covered by existing Rust crates (Symphonia). Video decoding is not currently available in Rust and would need porting or integration with non-Rust libraries (Category C: new implementation or binding).

## Security and Privacy

- **TLS/Certificates** – _Already in Rust:_ As mentioned, _rustls_ (Rust) can handle TLS. Certificate validation and stores can use _webpki_ (Rust). Root certificate bundles would be needed (existing crates or systems).
- **Sandboxing/Process Isolation** – Modern browsers often isolate tabs/processes for security. In Rust, one would rely on OS features (process/namespace) rather than a crate. Not a library task per se.
- **Content Security (XSS, CORS, CSP)** – The logic for enforcing same-origin, CORS, Content Security Policies, etc., must be implemented. No ready-made Rust library “solves” this; it’s browser logic. These must be coded in Rust (Category C).
- **URL Parsing & Normalization** – The _url_ crate (Rust) provides URL parsing. It covers RFCs for URL normalization.
- **Cryptography** – For any crypto (e.g. hashing, signatures), Rust has crates (e.g. _ring_, _sha2_, _hmac_).
- **Permissions and Privacy** – Browser permissions (geolocation, camera, etc.) would tie into OS – mostly custom logic.

**Classification:** The core crypto/TLS is covered by Rust crates (rustls/webpki/ring). Most “security” rules (CSP, same-origin, CSP, etc.) require custom Rust code (Category C).

## Other Components

- **Developer Tools** – No existing Rust library implements an inspector or console UI. Building devtools (DOM inspector, JS debugger UI) would require writing new Rust code (possibly using web-tech in UI or a GUI toolkit). Category C.
- **Extensions API** – Browser extension mechanisms (V8-based plugins or WebExtensions) are not in Rust. One would either forego them or implement a system from scratch. Category C.
- **Printing, Clipboard, Drag-drop** – These involve OS integration. Rust crates can help (e.g. _printpdf_, _clipboard_), but often one must bind OS APIs. New code needed.
- **Accessibility** – ARIA support, screen reader interfaces, etc., would need implementing (no specific Rust libs).
- **Media Capture (camera/microphone)** – Requires OS/hardware access (Rust crates might exist for camera, but likely OS calls needed).
- **Networking protocols (WebRTC)** – WebRTC stack (libwebrtc) has no pure-Rust version; would require porting or embedding C (Category B/C).

Overall, nearly all browser-specific features beyond the core engine must be written anew or integrated.

## Summary

In summary, almost every core browser subsystem has a Rust implementation or crate that can be used: HTML/CSS parsing (html5ever, rust-cssparser), layout and graphics (Servo’s style/layout and WebRender/Pathfinder), networking (hyper/rustls/quinn), fonts (font-kit), images (image crate), audio (Symphonia). These fall under **“Already in Rust”**.

The major missing pieces are chiefly the JavaScript engine (no full Rust JS engine yet – need to complete or port one), video decoding (no Rust codecs), and high-level UI/tooling (GUI, extensions, devtools). Those items must be written in Rust from scratch (Category C), or in some cases require careful porting of C/C++ code with no guarantee of zero bugs (Category B).

Thus an exhaustive component list includes:

- **UI & Windowing:** address bar, toolbars, menus, tabs (new Rust GUI code or bindings);
- **Browser Engine:** event loop, navigation logic (implement in Rust);
- **HTML Parser & DOM:** _html5ever_ (Rust); _rcdom_ (Rust) or custom DOM structures;
- **CSS Parser & Styling:** _rust-cssparser_ (Rust); style & layout engine (Rust, from Servo);
- **Layout Engine:** CSS layout algorithms (Rust);
- **Paint/Compositing:** WebRender (Rust); GPU context (glutin/wgpu Rust);
- **Canvas & SVG:** Pathfinder (Rust);
- **Font Handling:** font-kit (Rust); text shaping via pathfinder/font fallback;
- **Image Decoding:** _image/png/jpeg_ crates (Rust);
- **JavaScript Engine:** _Boa_ (Rust, incomplete) or new Rust implementation (need to write);
- **WebAssembly:** Wasmtime/wasmi (Rust);
- **Networking:** hyper/reqwest (Rust HTTP), rustls (TLS), quinn (QUIC), trust-dns (Rust);
- **Storage:** sled (Rust) or SQLite (via rusqlite), custom file caches;
- **Audio:** Symphonia (Rust); output (cpal/rodio Rust);
- **Video:** (no Rust, would need porting – e.g. FFMPEG);
- **Security:** rustls/webpki (TLS, Rust); crypto crates (Rust); content security logic (implement in Rust);
- **UI Backend:** form widget rendering (likely custom Rust using OS APIs);
- **Extensions, DevTools, Accessibility, Print, Drag/Drop, etc.:** mostly not in Rust – would require new implementation.

Each component should be classified accordingly: most parsers, rendering, networking and fonts are **(A)** (Rust-ready), whereas JS engine, video codecs, browser chrome, and advanced features are **(C)** (must implement in Rust). Those in category **(B)** (portable) include any heavy C/C++ libraries one might try to rewrite (e.g. WebRTC).

**Sources:** We rely on browser architecture overviews and existing Rust projects. For example, Will T. enumerates browser parts including UI, engine, renderer, networking, JS engine, UI backend, and storage. The Servo project and related crates demonstrate many Rust implementations: html5ever (HTML5 parser), rust-cssparser (CSS syntax), WebRender (GPU compositor), Pathfinder (vector graphics), font-kit (fonts), Symphonia (audio). A recent Rust browser (h5i) even uses Boa for JS. These examples show which components are already available in Rust and which are not.

## Prompt:

30/09/2026, 22:29:27

You said that Boa is not yet complete. What exactly is missing in Boa currently that is present in V8?

## Response:

30/09/2026, 22:29:33 · gpt-5-6-t-mini

> **Searched 4 websites**
>
> Worked for 31s

Yes. The important correction from my earlier answer is that **Boa is much closer to V8 on ECMAScript language coverage than my wording implied, but it is still far from V8 in execution technology, performance, completeness, and browser-runtime capabilities**.

As of **September 30, 2026**, the latest Boa release is **0.22**, released August 28, 2026. Boa reports **95.60% conformance on Test262**, the official ECMAScript conformance suite. Boa's own repository still describes it as an **experimental** JavaScript lexer/parser/interpreter. ([GitHub](https://github.com/boa-dev/boa/releases?utm_source=chatgpt.com))

The distinction becomes much clearer when you separate **"does Boa understand JavaScript correctly?"** from **"does Boa have the machinery V8 uses to execute modern JavaScript at Chrome-class speed and capability?"**

## 1. ECMAScript itself: Boa is already surprisingly far along

Boa currently has implementations for a very large portion of the standard library and language machinery, including:

- `Object`
- `Array`
- `String`
- `Number`
- `BigInt`
- `Symbol`
- `Map`
- `Set`
- `WeakMap`
- `WeakSet`
- `WeakRef`
- `Proxy`
- `Reflect`
- `RegExp`
- `Promise`
- generators
- async functions
- async generators
- `ArrayBuffer`
- `SharedArrayBuffer`
- typed arrays
- `DataView`
- `Atomics`
- `Intl`
- `Temporal`
- modules
- `eval`
- etc.

Those are actual Boa engine components, not merely planned features. ([Docs.rs](https://docs.rs/boa_engine/latest/boa_engine/builtins/index.html?utm_source=chatgpt.com))

Boa 0.22 also added or completed several things including `FinalizationRegistry`, `RegExp.escape()`, iterator helpers, additional Promise APIs, `Error.prototype.stack`, and more. ([GitHub](https://github.com/boa-dev/boa/releases?utm_source=chatgpt.com))

So **"Boa lacks modern JavaScript" is no longer an accurate description**.

The remaining ~4.4% of Test262 failures are the more precise indicator that Boa still has ECMAScript conformance gaps. However, the public release information I found does **not provide a clean authoritative feature-by-feature list of all 4.4% in one place**, so I would not invent a list and label it exhaustive. Boa itself directs users to its Test262 results for the authoritative breakdown. ([Boa JS](https://boajs.dev/conformance))

---

# 2. The enormous difference: execution engine

This is where Boa diverges radically from V8.

### Boa today

Boa's current execution pipeline is approximately:

**JavaScript → parser → AST → bytecompiler → bytecode → register-based VM → result**

Its `CodeBlock` contains bytecode, constants, bindings, exception information, inline caches, register information, etc. ([GitHub](https://github.com/boa-dev/boa/blob/main/docs/vm.md?utm_source=chatgpt.com))

It also has:

- a bytecode optimizer
- NaN-boxed `JsValue`
- inline caching for property access
- polymorphic inline-cache work
- a garbage collector
- a register-based VM

These are substantial pieces of a real JS engine. ([GitHub](https://github.com/boa-dev/boa/releases?utm_source=chatgpt.com))

### V8

V8 has a much deeper execution pipeline:

**JavaScript → AST → Ignition bytecode → Sparkplug → Maglev → TurboFan/Turboshaft → optimized native machine code**

Specifically:

| Layer                                     | Boa                  | V8                              |
| ----------------------------------------- | -------------------- | ------------------------------- |
| Parser                                    | ✅                   | ✅                              |
| AST                                       | ✅                   | ✅                              |
| Bytecode                                  | ✅                   | ✅                              |
| Interpreter                               | ✅ register VM       | ✅ Ignition                     |
| Basic optimization                        | ✅                   | ✅                              |
| Inline caches                             | ✅ property access   | ✅ extensive feedback machinery |
| Baseline native compiler                  | ❌                   | ✅ Sparkplug                    |
| Mid-tier optimizing JIT                   | ❌                   | ✅ Maglev                       |
| High-tier optimizing JIT                  | ❌                   | ✅ TurboFan/Turboshaft          |
| Dynamic type-feedback-driven optimization | limited              | ✅ extensive                    |
| Speculative optimization                  | very limited         | ✅                              |
| Deoptimization infrastructure             | not comparable to V8 | ✅                              |
| Dynamic PGO/tiering                       | not comparable       | ✅                              |

V8's official architecture documentation explicitly describes Sparkplug, Maglev, and TurboFan/Turboshaft as distinct compilation tiers. ([Chromium Code Reviews](https://chromium.googlesource.com/v8/v8.git/%2B/refs/heads/main/docs/overview.md))

This is **the single largest technical gap between Boa and V8**.

---

# 3. Boa does not currently have a production JIT

This is worth emphasizing because it changes the nature of the project.

In October 2025, a Boa maintainer explicitly said Boa did **not** support JIT compilation. ([GitHub](https://github.com/boa-dev/boa/discussions/4487?utm_source=chatgpt.com))

There has since been a **Cranelift JIT prototype** discussed in 2026, but that is a prototype branch rather than the production Boa execution architecture. The prototype supported only 109 of Boa's 196 opcodes and was explicitly still an experimental design exercise. ([GitHub](https://github.com/boa-dev/boa/discussions/4487?utm_source=chatgpt.com))

The Boa maintainers' discussion makes the gap particularly clear: what they ultimately want is not merely "compile bytecode to machine code", but a sophisticated optimizing compiler capable of bypassing the VM for hot code, performing speculative optimization, and using dynamic runtime information. ([GitHub](https://github.com/boa-dev/boa/discussions/4487?utm_source=chatgpt.com))

So your Rust browser project would eventually need something approximately equivalent to:

```text
                   ┌───────────────┐
                   │ JavaScript AST│
                   └───────┬───────┘
                           │
                      Bytecompiler
                           │
                     ┌─────▼─────┐
                     │  Bytecode  │
                     └─────┬─────┘
                           │
                       Interpreter
                           │
                ┌──────────▼──────────┐
                │ Runtime type/profile│
                │      feedback       │
                └──────────┬──────────┘
                           │
                  Tier-up / hotness
                           │
            ┌──────────────▼──────────────┐
            │   Baseline JIT             │
            │   ≈ Sparkplug              │
            └──────────────┬──────────────┘
                           │
                   More hot / stable
                           │
            ┌──────────────▼──────────────┐
            │      Mid-tier JIT            │
            │        ≈ Maglev              │
            └──────────────┬──────────────┘
                           │
                    Very hot code
                           │
            ┌──────────────▼──────────────┐
            │     Optimizing JIT           │
            │     ≈ Turboshaft             │
            └──────────────┬──────────────┘
                           │
                   native machine code
```

That entire latter half is a major engineering project.

---

# 4. Boa's inline caching is nowhere near V8's feedback machinery

Boa does have inline caches, which is important.

Its VM documentation says the cache is used for property accesses such as `GetPropertyByName` and `SetPropertyByName`, storing shape/property information so repeated accesses can avoid a full lookup. ([GitHub](https://github.com/boa-dev/boa/blob/main/docs/vm.md?utm_source=chatgpt.com))

But V8's runtime feedback system is much broader.

V8 collects type/profile information that can drive optimization of things such as:

- property access
- arithmetic
- comparisons
- calls
- object shapes
- call targets
- element kinds
- specialization decisions

The 2026 Boa JIT discussion itself specifically identifies **feedback vectors** as a major difference from Boa's current approach, noting that Boa's IC is currently focused on property access whereas V8 tracks much broader operation feedback. ([GitHub](https://github.com/boa-dev/boa/discussions/4487?utm_source=chatgpt.com))

That feedback machinery is essential to a modern JavaScript JIT.

---

# 5. Garbage collection: Boa has one, but V8's is considerably more sophisticated

This is another area where saying "Boa has GC, so it is basically equivalent" would be misleading.

Boa has its own Rust garbage collector, `boa_gc`. ([GitHub](https://github.com/boa-dev/boa))

V8 has a highly sophisticated generational collector, including:

- young generation
- old generation
- scavenging
- concurrent marking
- parallel work
- incremental/concurrent collection techniques
- compaction
- pointer compression
- sophisticated root/handle machinery
- tight integration with optimized JIT code

V8's documentation explicitly describes its generational GC and concurrent/parallel phases. ([GitHub](https://github.com/v8/v8/blob/main/docs/heap/garbage-collection.md?utm_source=chatgpt.com))

And this isn't merely about speed.

The GC must cooperate correctly with:

- machine-code stack frames
- optimized registers
- deoptimization
- inline caches
- weak references
- finalization
- object relocation
- concurrent compilation
- concurrent marking
- WebAssembly
- security mechanisms

Once you have a serious JIT, **GC becomes part of the compiler/runtime architecture**, not a separate subsystem.

---

# 6. V8's object representation and optimization machinery are much deeper

Boa has shapes/inline caches and NaN boxing.

V8 has:

- Maps/hidden classes
- descriptor arrays
- transitions
- element kinds
- inline caches
- feedback vectors
- optimized property access
- specialization
- allocation-site information
- object-layout optimizations

V8 explicitly documents Maps/"hidden classes" as a fundamental mechanism for accelerating property access. ([Chromium Code Reviews](https://chromium.googlesource.com/v8/v8.git/%2B/refs/heads/main/docs/overview.md))

Boa has the foundation, but not the decades of optimization machinery surrounding it.

---

# 7. WebAssembly is another major gap

This deserves correcting from my earlier answer.

I previously suggested that you could use Wasmtime/Wasmi and effectively treat WebAssembly as already solved. That was too simplistic.

**Boa itself does not currently provide a V8-equivalent WebAssembly engine.**

The Boa project has had a long-standing "Add WebAssembly support" discussion, and the project search still exposes that as an idea. ([GitHub](https://github.com/boa-dev/boa/discussions/categories/ideas?utm_source=chatgpt.com))

Meanwhile V8 has a full WebAssembly implementation, including:

- Wasm parser/validator
- compilation
- machine-code generation
- streaming compilation
- JS ↔ Wasm interop
- WebAssembly GC integration
- SIMD
- reference types
- multiple proposal features
- tiering and optimization

V8's architecture documentation explicitly describes its WebAssembly subsystem as a full implementation including streaming compilation and GC integration. ([Chromium Code Reviews](https://chromium.googlesource.com/v8/v8.git/%2B/refs/heads/main/docs/overview.md))

So for your browser:

**Boa + Wasmtime** would be a possible architectural combination, but it would not mean "Boa itself has WebAssembly".

You would need to build the **JavaScript ↔ WebAssembly integration layer** yourself.

---

# 8. ECMAScript `Intl` is present, but don't confuse that with V8's whole internationalization stack

Boa does now have an `Intl` implementation and integrates ICU4X components such as:

- locale
- datetime
- calendars
- collator
- plural rules
- list formatting
- case mapping
- normalization
- segmentation
- decimal formatting

Boa's dependencies show explicit ICU4X integration across these areas. ([GitHub](https://github.com/boa-dev/boa/blob/main/Cargo.toml?utm_source=chatgpt.com))

So this is **not a major "missing feature" anymore**.

There can still be conformance and data-version differences, but conceptually Boa has a legitimate ECMA-402 implementation.

---

# 9. Modules and async execution aren't the big missing pieces either

Boa already has:

- ECMAScript modules
- module loading infrastructure
- promises
- async functions
- async generators
- job queues
- microtasks
- `fetch`-related runtime work

Boa's 0.21/0.22 releases specifically mention substantial asynchronous-runtime and module-loader work. ([GitHub](https://github.com/boa-dev/boa/releases?utm_source=chatgpt.com))

The important distinction is that **a browser supplies the host environment**.

For example, V8 itself does not magically provide the DOM.

V8's own documentation explicitly notes that the DOM is supplied by the embedding application rather than by V8 itself. ([Chromium Code Reviews](https://chromium.googlesource.com/external/github.com/v8/v8.wiki/%2B/71511645ac05c5411b9dab09bc69f611cab5c0a5/Introduction.md?utm_source=chatgpt.com))

The same principle applies to Boa.

---

# 10. Browser APIs are mostly not part of Boa's core engine

This is where comparisons become confusing.

Suppose JavaScript executes:

```javascript
fetch("/api/data");
```

The ECMAScript engine doesn't inherently know how HTTP works.

The browser must provide an object/function implementing the Web API.

Boa has begun providing some runtime/Web API functionality in `boa_runtime`, and the project is actively migrating toward `boa_wintertc`. ([GitHub](https://github.com/boa-dev/boa/releases?utm_source=chatgpt.com))

But Chrome has an enormous host environment around V8:

```text
V8
│
├── DOM
├── Web IDL bindings
├── Fetch
├── Streams
├── WebSockets
├── WebRTC
├── Service Workers
├── IndexedDB
├── Storage
├── Canvas
├── WebGL
├── WebGPU
├── Media APIs
├── Permissions
├── File APIs
├── Clipboard
├── Notifications
├── Workers
├── Timers
├── Events
├── etc.
```

Those are **browser-engine / browser-runtime responsibilities**, not things V8 needs to contain.

So don't count all of that as "things Boa is missing from V8". It is mostly **things your browser must build around Boa**.

---

# 11. Debugging/inspection is another substantial difference

V8 has a mature debugging ecosystem and inspector protocol infrastructure.

V8's documentation includes dedicated support for:

- stack walking
- deoptimization
- interpreter/compiler tracing
- profiling
- heap diagnostics
- optimization tracing
- GC tracing
- inspector functionality

The official V8 documentation has extensive dedicated material for these areas. ([Chromium Code Reviews](https://chromium.googlesource.com/v8/v8/%2B/refs/heads/main/docs/?utm_source=chatgpt.com))

Boa has debugging facilities and test infrastructure, but it is not remotely equivalent to the Chrome/V8 developer-tools ecosystem.

For your browser you'd eventually need:

```text
Browser DevTools
   │
   ├── DOM Inspector
   ├── CSS Inspector
   ├── Console
   ├── Network Inspector
   ├── Sources/Debugger
   ├── Breakpoints
   ├── Watch expressions
   ├── Call stacks
   ├── CPU profiler
   ├── Memory profiler
   ├── Heap snapshots
   ├── Performance timeline
   ├── Rendering diagnostics
   └── Protocol
```

Much of that would have to be created around Boa.

---

# 12. V8 has a very mature startup/snapshot system

V8 has a heap snapshot/serialization system used to preinitialize engine state and reduce startup costs. ([Chromium Code Reviews](https://chromium.googlesource.com/v8/v8.git/%2B/refs/heads/main/docs/overview.md))

Boa doesn't have an equivalent V8-class snapshot architecture as a mature part of the engine.

That means your browser would eventually want something along the lines of:

```text
Prebuilt engine state
        ↓
Serialized startup snapshot
        ↓
Map/object/builtin initialization
        ↓
Fast creation of JS Realm
```

This matters significantly for browser startup performance.

---

# 13. V8's sandbox architecture is also a major difference

V8 has a dedicated sandbox security architecture intended to constrain the consequences of memory-corruption bugs within the engine. ([Chromium Code Reviews](https://chromium.googlesource.com/v8/v8.git/%2B/refs/heads/main/docs/overview.md))

Boa's Rust memory-safety story is fundamentally different.

Rust eliminates whole classes of memory-safety bugs, but it **does not automatically give you V8's sandbox architecture**, because sandboxing addresses a different problem.

Your browser would still need:

- process isolation
- renderer sandboxing
- site isolation
- capability restrictions
- IPC security
- OS sandboxing
- potentially a JavaScript-engine-specific containment strategy

---

# 14. Performance is arguably a larger gap than language conformance

This is an important point for your browser project.

A browser can pass a huge fraction of Test262 and still be nowhere near Chrome in real-world performance.

For example, the current Boa discussions show that significant performance work is still underway, and experimental JIT work is being explored precisely because interpreter execution remains a major bottleneck. ([GitHub](https://github.com/boa-dev/boa/discussions/4494?utm_source=chatgpt.com))

V8's design is fundamentally built around **tiered execution**:

```text
fast startup
    ↓
interpreter
    ↓
baseline native code
    ↓
mid-tier optimization
    ↓
aggressive optimization
    ↓
deoptimization when assumptions fail
```

That machinery is one of the reasons modern JavaScript applications can execute enormous amounts of dynamic code efficiently.

---

# So what exactly would you have to add to Boa?

For your hypothetical browser, I'd divide the gap into three categories.

### A. Boa already largely gives you this

| Capability                       | Boa |
| -------------------------------- | --- |
| JavaScript parser                | ✅  |
| AST                              | ✅  |
| ECMAScript bytecode compiler     | ✅  |
| Register VM                      | ✅  |
| Large ECMAScript builtin library | ✅  |
| Promises                         | ✅  |
| async/await                      | ✅  |
| generators                       | ✅  |
| async generators                 | ✅  |
| modules                          | ✅  |
| BigInt                           | ✅  |
| TypedArrays                      | ✅  |
| ArrayBuffer                      | ✅  |
| SharedArrayBuffer                | ✅  |
| Atomics                          | ✅  |
| WeakRef                          | ✅  |
| FinalizationRegistry             | ✅  |
| Proxy                            | ✅  |
| RegExp                           | ✅  |
| Intl                             | ✅  |
| Temporal                         | ✅  |
| Garbage collector                | ✅  |
| NaN-boxed values                 | ✅  |
| Basic inline caches              | ✅  |
| Web/runtime API beginnings       | ✅  |
| Test262 infrastructure           | ✅  |

Boa 0.22's release notes and API documentation support most of this inventory. ([GitHub](https://github.com/boa-dev/boa/releases?utm_source=chatgpt.com))

### B. Boa has the foundation, but is substantially behind V8

| Capability                          | Boa vs V8                           |
| ----------------------------------- | ----------------------------------- |
| Object shapes                       | ✅, less mature                     |
| Inline caching                      | ✅, much narrower                   |
| GC                                  | ✅, considerably less sophisticated |
| bytecode optimization               | ✅                                  |
| runtime profiling                   | partial                             |
| type feedback                       | much less extensive                 |
| startup snapshots                   | not comparable                      |
| debugging                           | partial                             |
| performance tooling                 | much less mature                    |
| compiler infrastructure             | much less mature                    |
| runtime optimization infrastructure | much less mature                    |

### C. Major things you would still need

| Capability                                  | Boa |
| ------------------------------------------- | --- |
| Production baseline JIT                     | ❌  |
| Sparkplug-equivalent                        | ❌  |
| Maglev-equivalent                           | ❌  |
| TurboFan/Turboshaft-equivalent              | ❌  |
| full speculative optimization               | ❌  |
| mature deoptimization system                | ❌  |
| V8-class dynamic PGO                        | ❌  |
| V8-class optimized machine-code pipeline    | ❌  |
| V8-class WebAssembly engine                 | ❌  |
| browser-grade Wasm ↔ JS integration         | ❌  |
| V8-class inspector/debugging infrastructure | ❌  |
| V8-class heap/snapshot machinery            | ❌  |

---

# The most important conclusion for your Rust-browser idea

There are actually **two completely different projects** hidden inside the sentence "make Boa equivalent to V8".

### Project 1 - Make Boa a fully conformant JavaScript engine

This is comparatively tractable.

Boa is already at **95.60% Test262 conformance**, so you're dealing with a remaining correctness/conformance problem rather than needing to invent JavaScript from scratch. ([GitHub](https://github.com/boa-dev/boa/releases?utm_source=chatgpt.com))

### Project 2 - Make Boa technologically comparable to V8

This is enormous.

You would need to create a complete modern optimizing JavaScript implementation:

```text
              ECMAScript
                  │
              Parser/AST
                  │
             Byte compiler
                  │
               Bytecode
                  │
             Interpreter
                  │
         Runtime type feedback
                  │
             ┌────▼────┐
             │ Baseline │
             │   JIT    │
             └────┬────┘
                  │
             ┌────▼────┐
             │Mid-tier │
             │   JIT   │
             └────┬────┘
                  │
             ┌────▼────┐
             │Optimizing│
             │   JIT    │
             └────┬────┘
                  │
          Native machine code
                  │
          ┌───────▼────────┐
          │ Sophisticated   │
          │ GC / deopt / IC │
          └────────────────┘
```

V8's current architecture explicitly has all of those execution tiers and supporting machinery. ([Chromium Code Reviews](https://chromium.googlesource.com/v8/v8.git/%2B/refs/heads/main/docs/overview.md))

And **that is just the JavaScript engine**.

Your browser still has to build the entire browser around it.

---

## One more correction to my original research

I would **not** now classify "JavaScript engine = Boa, therefore mostly solved" in your original three-way browser inventory.

A more accurate classification is:

> **Boa gives you a substantial Rust implementation of ECMAScript, but it does not give you a Chrome-class JavaScript execution engine.**

For your project I'd mark it:

**Boa → "Already written in Rust and reusable, but requires major continued development before it is suitable for a Chrome/Edge-class browser."**

That is substantially more accurate than simply putting it under "complete Rust component".

And the good news is that this means you **do not have to write JavaScript from scratch**. You'd be taking an existing serious Rust engine and turning it into a production browser engine, while concentrating your original work on the missing performance and browser-integration layers. ([GitHub](https://github.com/boa-dev/boa))

---

**Sources:**

- [Releases · boa-dev/boa · GitHub](https://github.com/boa-dev/boa/releases?utm_source=chatgpt.com)
- [boa_engine::builtins - Rust](https://docs.rs/boa_engine/latest/boa_engine/builtins/index.html?utm_source=chatgpt.com)
- [Conformance | Boa JS](https://boajs.dev/conformance)
- [V8 High-Level Overview](https://chromium.googlesource.com/v8/v8.git/%2B/refs/heads/main/docs/overview.md)

## Prompt:

30/09/2026, 22:37:51

Which of these gaps in Boa should be addressable first and comparatively easier to solve/make from scratch? Arrange these gaps in Boa in the order in which i should solve/implement them. Which gap should be solved after which one?

## Response:

30/09/2026, 22:37:56 · gpt-5-6-t-mini

I’ll order the Boa gaps by **dependency, implementation difficulty, and how much later work each unlocks**, rather than by perceived importance alone. The key is to avoid building an optimizer before Boa’s runtime semantics and profiling interfaces are stable.

A useful finding already changes the order: Boa’s own 2026 JIT experiment found that **GC tracing, property access, and function-call overhead** can dominate before bytecode dispatch does. That means “build a JIT immediately” is not the optimal first move.

> **Searched 2 websites**
>
> **Planning an implementation roadmap**
>
> I’m organizing the work by dependencies rather than feature prestige: first correctness and runtime foundations, then profiling and feedback, followed by baseline and optimizing tiers, deoptimization, snapshots, garbage-collection improvements, and finally advanced profiling.
>
> **Sequencing native execution**
>
> I’m placing garbage-collection maps and native-root handling before JIT code generation, even if basic bytecode-to-native compilation comes sooner. I’m separating copy-and-patch, baseline compilation, structured IR, deoptimization, and higher-tier optimization into dependent stages.
>
> **Planned implementation roadmap and sequenced native execution**
>
> **Searched 2 websites**
>
> Worked for 23s

Yes. For **your specific goal - taking Boa and turning it into the JS engine of a serious browser - I would not attack the gaps in the same order that V8's architecture evolved**.

The optimal order is based on three things:

1. what is easiest to implement safely,
2. what produces useful performance immediately,
3. what creates the infrastructure needed by the next stage.

Boa 0.22 already has a register VM, bytecode and bytecode/VM optimizations, polymorphic inline caches, NaN-boxed `JsValue`, GC, and ~95.6% Test262 conformance. ([GitHub](https://github.com/boa-dev/boa/releases?utm_source=chatgpt.com))

At the same time, its current open work and the 2026 JIT experimentation show that **function calls, property access, GC cost, and lack of broad runtime feedback are more immediate obstacles than raw interpreter dispatch**. ([GitHub](https://github.com/boa-dev/boa/discussions/4487?utm_source=chatgpt.com))

# The order I would use

```text
0.  Correctness / API stabilization
        ↓
1.  Cheap VM/runtime fast paths
        ↓
2.  GC and memory-management optimization
        ↓
3.  Runtime profiling + type feedback
        ↓
4.  Fast function-call machinery
        ↓
5.  Baseline JIT
        ↓
6.  Structured optimizing IR
        ↓
7.  Deoptimization infrastructure
        ↓
8.  Mid-tier optimizing JIT
        ↓
9.  Advanced GC/JIT integration
        ↓
10. Top-tier optimizing JIT
        ↓
11. Startup snapshots / code caching / advanced runtime tuning
```

There are also two **parallel tracks** that should not block this sequence:

```text
             ┌── WebAssembly engine ──┐
             │                         │
main road ───┼─────────────────────────┼──→ browser integration
             │                         │
             └── debugger/profiling ───┘
```

Let me go through the reasoning carefully.

---

# 0. First: eliminate correctness and architectural instability

This isn't technically a "V8 performance gap", but I would put it at **step zero**.

Boa still has open correctness issues in 2026, including VM and parser bugs, and the project is actively working on API stabilization and removing panic paths. ([GitHub](https://github.com/boa-dev/boa/issues))

For example, the current issue tracker contains things such as:

- incorrect register forwarding behavior,
- parser conformance bugs,
- runtime overflow bugs,
- API stabilization,
- remaining unsafe/runtime concerns.

([GitHub](https://github.com/boa-dev/boa/issues))

### Why before optimization

You do not want:

```text
buggy VM
   ↓
JIT
   ↓
optimized buggy semantics
   ↓
impossible debugging nightmare
```

Instead:

```text
correct interpreter
       ↓
correct fast paths
       ↓
correct JIT
```

For a browser, **the interpreter should remain the semantic reference implementation**.

Every later tier should be tested against it.

---

# 1. Optimize the existing VM before creating a JIT

This is the first actual performance stage, and it is comparatively easy.

Boa's current bytecode VM already has inline caches. The current documentation describes a cache for property loads/stores keyed by object shape. ([GitHub](https://github.com/boa-dev/boa/blob/main/docs/vm.md?utm_source=chatgpt.com))

There are also already-concrete 2026 performance efforts around things like:

- removing `GcRefCell` overhead from inline caches,
- faster `OrdinaryGet`,
- dense-array paths,
- other VM/runtime hot paths.

([GitHub](https://github.com/boa-dev/boa/pulls?utm_source=chatgpt.com))

## Implement in roughly this order

### 1A. Property access fast paths

Start here:

```text
GetPropertyByName
SetPropertyByName
GetPropertyByValue
SetPropertyByValue
```

Then:

```text
own-property fast path
prototype fast path
array-index fast path
dense-array path
```

Boa has already moved from monomorphic IC toward polymorphic IC in 0.22, so you are extending an architecture that is already there rather than inventing one. ([GitHub](https://github.com/boa-dev/boa/releases?utm_source=chatgpt.com))

### 1B. Function-call fast paths

The 2026 Cranelift prototype found ordinary function calls to be a significant bottleneck: its `jit_call` still went through the full `function_call` machinery, and the author specifically identified a fast ordinary-function path as one of the clearest next improvements. ([GitHub](https://github.com/boa-dev/boa/discussions/4487?utm_source=chatgpt.com))

You want something like:

```text
GenericCall()
     │
     ├── builtin?
     ├── bound?
     ├── proxy?
     ├── constructor?
     └── ordinary JS function?
              ↓
         FAST PATH
```

The ordinary-JS-function case should avoid unnecessary general machinery.

### 1C. Numeric operations

Add fast paths for common operations:

```text
i32 + i32
i32 - i32
i32 * i32
i32 comparisons
bitwise operations
```

with fallback:

```text
fast path
   ↓
tag/type check fails
   ↓
generic ECMAScript semantics
```

This is exactly the kind of optimization already demonstrated successfully in the experimental Boa JIT, which inlined NaN-boxing-aware integer paths. ([GitHub](https://github.com/boa-dev/boa/discussions/4487?utm_source=chatgpt.com))

---

# 2. Improve the GC before serious JIT work

This one is easy to underestimate.

The 2026 Boa JIT prototype discovered that **GC tracing accounted for roughly 10-16% of its V8-benchmark time** and caused some workloads not to improve despite JIT compilation. ([GitHub](https://github.com/boa-dev/boa/discussions/4487?utm_source=chatgpt.com))

So:

```text
JIT doesn't automatically fix performance
```

If:

```text
20% dispatch
15% property access
15% GC
10% calls
40% everything else
```

and your JIT removes only the 20% dispatch component, you haven't transformed the browser engine.

## What I would do

First profile the GC.

Then tackle:

```text
allocation cost
↓
marking cost
↓
tracing cost
↓
write barriers
↓
root scanning
↓
object metadata
```

Boa itself has an open issue investigating different GC API models, so this is not merely theoretical work. ([GitHub](https://github.com/boa-dev/boa/issues?utm_source=chatgpt.com))

### Important distinction

Do **not** immediately attempt to recreate V8's entire concurrent generational collector.

First make Boa's existing collector cheaper.

Then redesign it for JIT integration.

---

# 3. Build a real runtime feedback system

This is the most important architectural preparation before a serious optimizing JIT.

Today, Boa's IC machinery primarily records information about property access. Boa maintainers and contributors explicitly identified the lack of something analogous to V8's broad feedback vector as a major missing piece. ([GitHub](https://github.com/boa-dev/boa/discussions/4487?utm_source=chatgpt.com))

V8's feedback infrastructure records information about many different operation classes, including:

- calls,
- property loads,
- global loads/stores,
- keyed loads/stores,
- binary operations,
- comparisons,
- closures,
- literals,
- `instanceof`,
- and more. ([Chromium Code Reviews](https://chromium.googlesource.com/v8/v8/%2B/ef4e2ab7bd75eae60e8ab2e7757e0772c4d00efd/src/type-feedback-vector.h?utm_source=chatgpt.com))

## Build a Boa feedback vector

Conceptually:

```text
CodeBlock
  │
  ├── bytecode
  ├── constants
  ├── ICs
  └── feedback[]
          │
          ├── LoadProperty
          ├── StoreProperty
          ├── Call
          ├── BinaryOp
          ├── Compare
          ├── GlobalLoad
          ├── GlobalStore
          ├── ArrayAccess
          └── ...
```

And each slot can evolve:

```text
UNINITIALIZED
      ↓
MONOMORPHIC
      ↓
POLYMORPHIC
      ↓
MEGAMORPHIC
```

You don't need V8's exact design.

You need the **concept**:

> "This operation has repeatedly seen these kinds of inputs."

That becomes the fuel for optimization.

---

# 4. Add proper execution hotness/tiering counters

This is comparatively easy once the feedback structure exists.

You need:

```text
function invocation count
loop backedge count
OSR counters
hotness state
```

For example:

```text
interpret
   │
   ├── 10 calls
   │
   ↓
baseline candidate

   │
   ├── 500 calls
   │
   ↓
mid-tier candidate

   │
   ├── 3000 calls
   │
   ↓
top-tier candidate
```

V8 currently uses an explicit multi-tier strategy and interruption/interrupt-budget mechanisms to decide when functions should move between tiers. ([Chromium Code Reviews](https://chromium.googlesource.com/v8/v8/%2Bshow/main/docs/runtime/tiering.md?utm_source=chatgpt.com))

Boa already has runtime-limit checking inside calls and loops, so the conceptual infrastructure for execution counters is not foreign to the VM. ([GitHub](https://github.com/boa-dev/boa/blob/main/docs/vm.md?utm_source=chatgpt.com))

---

# 5. Only now build a baseline JIT

This is where I'd resist the temptation to jump directly into "TurboFan but in Rust".

Don't.

The easiest useful JIT is:

> **compile Boa bytecode into native code without trying to be clever.**

V8's Sparkplug is explicitly built around this philosophy: it takes interpreter bytecode and translates it directly to machine code with little optimization, primarily eliminating interpreter dispatch overhead. ([Chromium Code Reviews](https://chromium.googlesource.com/v8/v8.git/%2B/refs/heads/main/docs/compiler/sparkplug/compiler-sparkplug.md?utm_source=chatgpt.com))

## Your first JIT should look like

```text
Boa bytecode
      ↓
opcode translator
      ↓
native code
```

For example:

```text
ADD
 ↓
load operands
 ↓
check common numeric case
 ↓
native add
 ↓
fallback helper
```

You don't need:

- sophisticated SSA
- global optimization
- speculative inlining
- escape analysis
- sophisticated LICM
- aggressive vectorization

yet.

---

# 6. I would actually start the baseline JIT with copy-and-patch or a very simple Cranelift backend

Boa's 2026 JIT discussion is extremely relevant here.

A Cranelift-based prototype reached measurable speedups and demonstrated that **you can add a JIT incrementally rather than redesigning the entire engine in one shot**. The prototype supported 109 of 196 opcodes, with unsupported functions remaining interpreted. ([github.com](https://github.com/boa-dev/boa/discussions/4487?utm_source=chatgpt.com))

The same discussion also explored copy-and-patch as an even lighter-weight baseline tier. ([github.com](https://github.com/boa-dev/boa/discussions/4487?utm_source=chatgpt.com))

So you could do:

```text
Phase A:
copy-and-patch-ish fast templates

Phase B:
Cranelift baseline JIT

Phase C:
optimized Cranelift/own IR
```

That is far more manageable than attempting a top-tier optimizer immediately.

---

# 7. Build the structured IR after the baseline JIT, not before

This is one place where architectural sequencing matters.

Boa's current bytecode is a byte stream with operands. Its maintainers have discussed introducing a more structured IR for optimization, and there has been debate about whether the current bytecode itself should serve as the optimization IR or whether another representation should be introduced. ([GitHub](https://github.com/boa-dev/boa/discussions/4487?utm_source=chatgpt.com))

I would eventually make:

```text
JavaScript
    ↓
Parser
    ↓
Boa frontend IR
    ↓
    ├──────────────→ Bytecode
    │
    └──────────────→ Optimizing compiler
```

rather than forcing the optimizing compiler to understand every implementation detail of the serialized bytecode.

### Why not do this first

Because your first baseline JIT can prove:

- calling convention
- value representation
- executable-memory model
- runtime-helper ABI
- exception handling
- object access
- code patching
- GC interaction

without simultaneously designing the final optimizer.

You'll learn a lot from the baseline implementation.

---

# 8. Then build deoptimization

This is the point where things become substantially harder.

A serious optimizing JIT makes assumptions:

```text
x is probably Int32
obj probably has Shape A
foo probably refers to function F
array is probably packed
```

Eventually:

```javascript
x = "hello";
```

and your assumption becomes false.

Your native code must then say:

```text
optimized assumption failed
        ↓
reconstruct interpreter state
        ↓
resume bytecode
```

That's **deoptimization**.

V8 explicitly has machinery for transitioning optimized code back to interpreter frames, including deoptimization frames containing parameters, registers and accumulator state. ([Chromium Code Reviews](https://chromium.googlesource.com/v8/v8.git/%2B/refs/heads/main/docs/compiler/maglev/compiler-maglev.md?utm_source=chatgpt.com))

## This is the real dependency

You cannot build a serious speculative optimizer without:

```text
feedback
   ↓
speculation
   ↓
guards
   ↓
deoptimization
```

So:

```text
feedback vector
       ↓
optimized JIT
       ↓
deoptimization
```

not the other way around.

---

# 9. Build the mid-tier optimizer

Now you can implement something roughly analogous to Maglev.

V8 describes Maglev as a deliberately faster optimizing tier between baseline compilation and the expensive top-tier compiler. It uses a graph/SSA-style IR and performs things like:

- type guarding,
- inlining,
- selected optimizations,
- lazy deoptimization.

([Chromium Code Reviews](https://chromium.googlesource.com/v8/v8.git/%2B/refs/heads/main/docs/compiler/maglev/compiler-maglev.md?utm_source=chatgpt.com))

For Boa, I'd aim for:

```text
Bytecode
   ↓
SSA-like Boa IR
   ↓
constant folding
   ↓
type specialization
   ↓
basic inlining
   ↓
dead code elimination
   ↓
simple range analysis
   ↓
native code
```

This is the first point at which you're building a genuine optimizing compiler.

---

# 10. Only after that: redesign the GC around JIT execution

This stage is difficult enough that I would deliberately postpone the _deep_ GC redesign until the shape of your native execution architecture is known.

Once optimized native code exists, the GC has to understand:

```text
native registers
native stack frames
boxed/unboxed values
GC roots
relocated objects
safepoints
background compilation
deoptimization metadata
```

At that point you'll likely need:

```text
Safepoint table
      +
GC root map
      +
JIT stack maps
      +
write barriers
      +
precise tracing
```

This is much closer to the sort of runtime/compiler coupling seen in V8.

---

# 11. Then build your top-tier optimizer

This is the truly enormous part.

Your architecture might eventually look like:

```text
                 JavaScript
                     │
                  Parser
                     │
                 Frontend IR
                     │
              ┌──────┴──────┐
              │             │
           Bytecode       Optimizer
              │             │
         Interpreter     SSA/CFG IR
              │             │
              │       ┌─────┴─────────┐
              │       │               │
              │     Mid-tier       Top-tier
              │       │               │
              │       └──────┬────────┘
              │              │
              └──────┬───────┘
                     │
               Native code
                     │
                 Runtime
                     │
                Garbage GC
```

V8's current architecture similarly has:

- Ignition,
- Sparkplug,
- Maglev,
- TurboFan/Turboshaft,

with tiering between them. ([Chromium Code Reviews](https://chromium.googlesource.com/v8/v8.git/%2B/refs/heads/main/docs/overview.md?utm_source=chatgpt.com))

Your top tier would need progressively more sophisticated optimization:

```text
inlining
constant propagation
type specialization
range analysis
escape analysis
common-subexpression elimination
dead-code elimination
loop-invariant code motion
bounds-check elimination
representation specialization
allocation optimization
global value numbering
etc.
```

You should treat this as its **own compiler project**.

---

# Where WebAssembly belongs

I would **not put Wasm on the critical dependency chain**.

Boa's project currently still lists adding WebAssembly support as an idea, so this remains an independent engine-sized gap. ([GitHub](https://github.com/boa-dev/boa/discussions/categories/ideas?utm_source=chatgpt.com))

You can build:

```text
Boa JS engine
       │
       ├── JavaScript JIT
       │
       └── Wasm engine
```

in parallel.

A Rust Wasm engine such as Wasmtime/Wasmi could potentially handle the Wasm execution side, but your browser still has to integrate:

```text
JavaScript
   ↕
Wasm objects
   ↕
Wasm memory
   ↕
Wasm tables
   ↕
host functions
```

That integration should happen after your basic JS runtime architecture has stabilized.

---

# Where debugger/inspector belongs

Also parallel.

Don't wait until the end.

Implement a minimal debugging interface fairly early because it is incredibly useful for finding optimizer/compiler bugs.

A good sequence is:

```text
interpreter
  ↓
source locations
  ↓
stack traces
  ↓
breakpoints
  ↓
bytecode debugger
  ↓
baseline-JIT debugger
  ↓
deoptimization-aware debugger
  ↓
heap profiler
```

Boa 2026 discussions still contain work around backtraces and debugging APIs, so this isn't currently at Chrome maturity. ([GitHub](https://github.com/boa-dev/boa/discussions?utm_source=chatgpt.com))

---

# The complete dependency graph

This is the version I would actually use as an engineering roadmap:

```text
                    ┌──────────────────────┐
                    │ 0. Correctness       │
                    │ + Test262 stability  │
                    └──────────┬───────────┘
                               │
                    ┌──────────▼───────────┐
                    │ 1. VM fast paths     │
                    │ IC / arrays / calls  │
                    └──────────┬───────────┘
                               │
                    ┌──────────▼───────────┐
                    │ 2. GC optimization   │
                    │ allocation/tracing   │
                    └──────────┬───────────┘
                               │
                    ┌──────────▼───────────┐
                    │ 3. Feedback system   │
                    │ types / calls / ICs  │
                    └──────────┬───────────┘
                               │
                    ┌──────────▼───────────┐
                    │ 4. Hotness/tiering   │
                    │ counters / OSR       │
                    └──────────┬───────────┘
                               │
                    ┌──────────▼───────────┐
                    │ 5. Baseline JIT      │
                    │ copy+patch/Cranelift │
                    └──────────┬───────────┘
                               │
                    ┌──────────▼───────────┐
                    │ 6. Structured IR    │
                    │ SSA/CFG representation│
                    └──────────┬───────────┘
                               │
                    ┌──────────▼───────────┐
                    │ 7. Deoptimization    │
                    │ guards + frame maps  │
                    └──────────┬───────────┘
                               │
                    ┌──────────▼───────────┐
                    │ 8. Mid-tier JIT      │
                    │ Maglev-like          │
                    └──────────┬───────────┘
                               │
                    ┌──────────▼───────────┐
                    │ 9. JIT-aware GC      │
                    │ safepoints/root maps │
                    └──────────┬───────────┘
                               │
                    ┌──────────▼───────────┐
                    │10. Top-tier JIT      │
                    │ TurboFan-like        │
                    └──────────┬───────────┘
                               │
                    ┌──────────▼───────────┐
                    │11. Snapshot/cache/   │
                    │ advanced tuning      │
                    └──────────────────────┘

        ┌───────────────────────────────┐
        │ Parallel: WebAssembly         │
        └───────────────────────────────┘

        ┌───────────────────────────────┐
        │ Parallel: Debugger/Profiler   │
        └───────────────────────────────┘
```

# What is actually "easy", "medium", and "hard"

| Boa gap                               | Relative difficulty | Recommended position |
| ------------------------------------- | ------------------: | -------------------: |
| IC implementation cleanup             |                Easy |                    1 |
| Array/dense-element fast paths        |         Easy-Medium |                    1 |
| Ordinary function-call fast path      |              Medium |                    1 |
| Numeric VM fast paths                 |              Medium |                    1 |
| GC allocation/tracing optimizations   |              Medium |                    2 |
| Invocation counters                   |                Easy |                  3/4 |
| Feedback slots                        |              Medium |                    3 |
| Broader type feedback                 |         Medium-Hard |                    3 |
| Baseline machine-code generation      |                Hard |                    5 |
| Copy-and-patch JIT                    |                Hard |                    5 |
| Cranelift JIT                         |                Hard |                    5 |
| Structured IR                         |                Hard |                    6 |
| Deoptimization                        |           Very hard |                    7 |
| Mid-tier compiler                     |           Very hard |                    8 |
| JIT-aware GC/safepoints               |           Very hard |                    9 |
| Top-tier optimizer                    |             Extreme |                   10 |
| V8-class runtime/compiler integration |             Extreme |                  10+ |

---

# One particularly important strategic point

**Do not make "V8 parity" your first milestone.**

Make the milestones cumulative:

### Milestone 1

```text
Boa interpreter
+
better IC
+
better calls
+
better GC
```

### Milestone 2

```text
Boa interpreter
+
feedback system
+
baseline JIT
```

At this point you already have a genuinely useful faster engine.

### Milestone 3

```text
baseline JIT
+
SSA IR
+
deoptimization
+
mid-tier optimizer
```

Now you're approaching the architecture of a serious modern JS engine.

### Milestone 4

```text
mid-tier
+
advanced GC
+
top-tier optimizer
+
sophisticated runtime feedback
```

Now you're entering **V8-class engine architecture** territory.

That is a far better roadmap than trying to reproduce TurboFan first.

The interesting part is that the current Boa project has already moved into exactly the foundations you'd want: v0.22 has bytecode/VM optimizations and polymorphic ICs, while the 2026 JIT prototype has demonstrated a workable Cranelift path and identified the concrete bottlenecks that should be fixed first. ([GitHub](https://github.com/boa-dev/boa/releases?utm_source=chatgpt.com))

For your browser specifically, I would therefore make **`better VM → better GC → feedback vector → baseline JIT → IR → deopt → mid-tier → JIT-aware GC → top-tier`** the core Boa roadmap. That sequence minimizes rework while giving you usable performance improvements at every stage.

---

**Sources:**

- [Releases · boa-dev/boa · GitHub](https://github.com/boa-dev/boa/releases?utm_source=chatgpt.com)
- [src/type-feedback-vector.h - v8/v8 - Git at Google](https://chromium.googlesource.com/v8/v8/%2B/ef4e2ab7bd75eae60e8ab2e7757e0772c4d00efd/src/type-feedback-vector.h?utm_source=chatgpt.com)

## Prompt:

30/09/2026, 23:24:51

"Boa still has open correctness issues in 2026, including VM and parser bugs, and the project is actively working on API stabilization and removing panic paths. " All these bugs you listed have been found out by many different people over a long time. how can i guarantee that fixing even these bugs that you listed with completely fix correctness and architectural instability issues. Since the code base is so big, there might be other bugs maybe some critical once that havent even been discovered yet? And since you put "having a correct interpreter" as a pre-requisite for further steps, and any bugs here could dramatically magify into a nightmare debugging situation later down the line, how will i be able to completely discover and fix all possible bugs that exist in this repo?

## Response:

30/09/2026, 23:24:59 · gpt-5-6-t-mini

> **Assessing interpreter correctness**
>
> I’m evaluating the claim carefully: complete bug discovery isn’t achievable through testing alone. I’ll frame a rigorous plan using specification conformance, differential testing, fuzzing, formal methods, invariants, regression suites, and transparent limitations.
>
> **Assessed interpreter correctness**
>
> **Building trustworthy semantics**
>
> I’m separating absolute proof from strong engineering confidence. I’d freeze semantics gradually, compare multiple independent engines, fuzz aggressively, enforce invariants, minimize failures, and require every discovered discrepancy to become a permanent regression test.
>
> I’m treating the interpreter as a reference model, not unquestionable truth. JIT optimization would proceed only when it repeatedly matches that model and independent expectations across broad, adversarial programs.
>
> **Searched 17 websites**
>
> Worked for 37s

Your concern is exactly right, and I would actually revise one part of my previous roadmap.

I said **“have a correct interpreter first”**. That can sound like there is some point where you inspect Boa, fix the currently known bugs, run the tests, and can then declare:

> “The interpreter is now correct. It is safe to build the JIT.”

You cannot establish that merely by fixing the known issues. In fact, **you cannot prove that an ordinary implementation as large as Boa contains no undiscovered bugs by testing it**, no matter how much testing you do.

The right goal is different:

> **Build an assurance system in which undiscovered bugs are continuously hunted, correctness is independently checked from several directions, and every later optimization is mechanically checked against a trusted semantic baseline.**

That is how I would approach Boa.

Boa itself currently calls itself an **“experimental JavaScript lexer, parser and interpreter”**; its v0.22 release reports 95.60% Test262 conformance and explicitly includes work on VM bugs, parser bugs, GC, performance, and removal of panics. The repository currently has hundreds of contributors and a substantial issue/PR backlog, which is exactly why looking only at the existing issue tracker is insufficient. ([GitHub](https://github.com/boa-dev/boa))

---

# 1. First, the uncomfortable answer: you cannot guarantee that all bugs have been found

There are three very different claims:

### Claim A

> “I fixed every bug currently known to me.”

Possible.

### Claim B

> “I have extremely high confidence that there are no important undiscovered bugs in the tested scope.”

Possible, and this is the realistic target for a production browser.

### Claim C

> “There exists no bug anywhere in the implementation.”

You cannot establish that through conventional testing.

And even **formal verification** doesn't magically solve the entire problem unless you formally specify and prove the entire implementation against a complete formal model.

There are several reasons.

A huge engine has:

```text
millions of possible inputs
×
millions of possible internal states
×
different host environments
×
different memory layouts
×
different timing/interleavings
×
different resource limits
×
different optimization decisions
×
different GC states
```

The state space is astronomical.

And some classes of program properties are undecidable in general.

Even Test262, despite being the official ECMAScript conformance suite and containing more than 50,000 test files as of 2025, explicitly says its coverage is **not complete** and that tests themselves can contain omissions or errors. ([GitHub](https://github.com/tc39/test262?utm_source=chatgpt.com))

So you should **never define your project milestone as “all bugs eliminated.”**

---

# 2. But there is a very important trick that solves your “bug amplification” problem

The crucial architectural idea is:

> **Do not make your first JIT the thing that defines JavaScript semantics.**

Instead:

```text
                       JavaScript source
                              │
                         Parser / AST
                              │
                           Bytecode
                              │
                     ┌────────▼────────┐
                     │ Reference VM    │
                     │ (Boa interpreter)│
                     └────────┬────────┘
                              │
                         JS semantics
                              │
              ┌───────────────┴───────────────┐
              │                               │
        Baseline JIT                    Optimizing JIT
              │                               │
              └───────────────┬───────────────┘
                              │
                        Native execution
```

The interpreter becomes the **semantic reference**, not necessarily a perfectly bug-free artifact.

Then every optimization has to satisfy:

```text
JIT(program) ≡ Interpreter(program)
```

for every behavior that is defined in the relevant test scope.

This changes the problem dramatically.

Suppose your interpreter contains a bug:

```javascript
x = weird_object;
x.foo;
```

and Boa incorrectly returns `42` instead of `17`.

A baseline JIT that also returns `42` isn't necessarily introducing a new discrepancy.

Then you compare the interpreter against **external oracles**:

```text
Boa interpreter
       ↓
   differs from
       ↓
V8 / SpiderMonkey / JavaScriptCore
       ↓
or Test262 expected behavior
       ↓
find original bug
```

So instead of requiring:

> “The interpreter must be mathematically perfect before I can touch anything else.”

you can require:

> “Every new execution tier must agree with the interpreter, while the interpreter itself is continuously compared against independent specifications and implementations.”

That is much more achievable.

---

# 3. Think of correctness as a pyramid, not a checklist

This is the architecture I would use.

```text
                    ┌─────────────────────┐
                    │ Formal proofs       │
                    │ of critical pieces  │
                    └──────────┬──────────┘
                               │
                    ┌──────────▼──────────┐
                    │ Independent audits  │
                    └──────────┬──────────┘
                               │
                    ┌──────────▼──────────┐
                    │ Differential fuzzing │
                    └──────────┬──────────┘
                               │
                    ┌──────────▼──────────┐
                    │ Coverage fuzzing    │
                    └──────────┬──────────┘
                               │
                    ┌──────────▼──────────┐
                    │ Test262 + WPT       │
                    └──────────┬──────────┘
                               │
                    ┌──────────▼──────────┐
                    │ Unit/integration    │
                    │ regression tests    │
                    └─────────────────────┘
```

Each layer catches classes of bugs the layer below it can miss.

---

# 4. Layer 1 - Exhaustive static inventory of the codebase

Before fixing anything, I would **map the entire Boa engine**.

Not just:

```text
parser/
vm/
builtins/
```

but every path through:

```text
parser
AST
compiler
bytecode
VM
values
objects
property lookup
prototype chain
functions
closures
environments
GC
strings
regex
Intl
modules
job queue
runtime
host bindings
FFI
unsafe code
threading
serialization
```

Then classify every component by:

```text
semantic criticality
memory-safety criticality
security criticality
concurrency
unsafe usage
external dependencies
test coverage
spec coverage
```

You want a dependency graph such as:

```text
Parser
  ↓
AST
  ↓
Bytecompiler
  ↓
Bytecode
  ↓
VM
  ↓
Objects / Values / GC
```

and independently:

```text
Parser → syntax semantics
Objects → property semantics
GC → object lifetime
Runtime → host semantics
```

This lets you identify **single points of catastrophic failure**.

For example:

```text
JsValue representation
          ↓
     used everywhere
```

is much more dangerous than some isolated builtin.

---

# 5. Layer 2 - Make the existing test suite vastly stronger

Boa already has:

- its own test suite,
- Test262 integration,
- WPT-related testing,
- benchmarks.

Its contributor documentation explicitly shows how to run the Boa suite and the full ECMAScript suite. ([GitHub](https://github.com/boa-dev/boa/blob/main/CONTRIBUTING.md?utm_source=chatgpt.com))

But don't merely run those tests.

You should build a **regression database**.

Every discovered bug becomes:

```text
bug
 ↓
minimal reproducer
 ↓
unit test
 ↓
integration test if appropriate
 ↓
fuzz corpus seed
 ↓
permanent regression case
```

This creates the important property:

```text
Bug found once
      ↓
can never silently return
```

---

# 6. Layer 3 - Test262 is necessary, but absolutely not sufficient

This is an extremely important distinction.

Test262 tests ECMAScript conformance. It covers a huge portion of the algorithms and grammar productions of ECMA-262 and ECMA-402, but TC39 explicitly says its coverage is not complete. ([GitHub](https://github.com/tc39/test262?utm_source=chatgpt.com))

So your gate should not be:

```text
95.6% → good enough
```

Eventually the goal should be more like:

```text
all applicable Test262 tests
        ↓
pass
        +
every failure explained
        +
every unsupported feature explicitly classified
        +
no hidden skip mechanism
```

And critically:

### Don't optimize around the score.

A test harness should fail if:

```text
test crashed
test timed out
test could not execute
test was silently skipped
```

rather than allowing those to disappear from the denominator.

Boa's existing tester already records panic information in logs, which is useful infrastructure to build upon. ([GitHub](https://github.com/boa-dev/boa/blob/main/CONTRIBUTING.md?utm_source=chatgpt.com))

---

# 7. Layer 4 - Differential testing is your biggest weapon

This is probably the single most important technique for your problem.

You have several independent JS implementations:

```text
Boa
V8
SpiderMonkey
JavaScriptCore
ChakraCore / other engines where useful
```

Generate the same program and execute it everywhere.

For example:

```text
program:
    let a = ...
    let b = ...
    ...
```

Then normalize the result:

```text
result
exception
observable side effects
property values
iteration order
```

and compare.

You can discover:

```text
Boa ≠ V8
Boa ≠ SpiderMonkey
Boa = JSC
```

which suggests a Boa bug, although you then still need to consult the specification rather than blindly voting by majority.

### Why three engines are better than one

Suppose:

```text
Boa = V8
Boa ≠ SpiderMonkey
```

You investigate.

But suppose:

```text
Boa = V8 = SpiderMonkey
JSC ≠ everyone
```

Now JSC may be wrong.

This is why differential testing is powerful but **not itself a formal oracle**.

The best hierarchy is:

```text
ECMAScript specification
       ↑
       │
Test262
       ↑
       │
multiple independent engines
       ↑
       │
Boa
```

---

# 8. Use a specialized JS fuzzer rather than just random strings

This is where something like **Fuzzilli** becomes extremely valuable.

Fuzzilli was specifically built to generate syntactically and semantically interesting JavaScript for dynamic language interpreters, with mutation and generation strategies designed to reach deep execution states. It is used specifically to find engine crashes and behavioral differences. ([GitHub](https://github.com/googleprojectzero/fuzzilli/blob/main/Docs/HowFuzzilliWorks.md?utm_source=chatgpt.com))

Randomly generating:

```text
!!!![]+{}(){}[]...
```

is nowhere near enough.

You want generated programs involving combinations such as:

```text
Proxy
+
getter
+
prototype mutation
+
async function
+
Promise
+
microtask
+
WeakRef
+
GC
+
exception
+
eval
+
closure
```

because browser-engine bugs often exist **in interactions between features**.

That is precisely where ordinary unit tests become weak.

---

# 9. Build several different fuzzers, not one

I would create at least these:

## Fuzzer A - Parser fuzzer

Input:

```text
arbitrary bytes
```

Target:

```text
lexer
parser
AST builder
```

Properties:

```text
never UB
never panic
never infinite-loop
never corrupt state
```

This finds malformed-input bugs.

---

## Fuzzer B - Semantic JavaScript fuzzer

Generate valid JavaScript.

Then:

```text
Boa interpreter
vs
V8
vs
SpiderMonkey
vs
JSC
```

Compare behavior.

---

## Fuzzer C - VM bytecode fuzzer

Instead of generating JavaScript, directly generate **valid Boa bytecode**.

This is extremely valuable because it bypasses the parser.

You can discover:

```text
invalid stack/register assumptions
bad operand decoding
incorrect exception handling
register aliasing problems
bad jump targets
incorrect environment handling
```

---

## Fuzzer D - JIT differential fuzzer

Once the JIT exists:

```text
program
   ↓
Boa interpreter ───┐
                   ├── compare
Boa baseline JIT ──┤
                   │
Boa optimized JIT ─┘
```

This is more important than comparing only with V8.

You now have your own **trusted internal oracle**.

---

## Fuzzer E - GC stress fuzzer

Generate programs specifically designed to stress:

```text
allocation
collection
weak references
finalization
closures
prototype chains
large object graphs
cycles
ephemerons
```

You want something like:

```text
allocate millions
mutate graph
drop references
trigger GC
allocate again
repeat
```

under sanitizers and debug assertions.

---

# 10. Use metamorphic testing

This is one of the techniques I think would be especially valuable for Boa.

A **metamorphic test** doesn't require knowing the exact expected output.

Instead, you know two programs should have the same behavior.

For example:

```javascript
x = (1 + 2) * 3;
```

could be transformed into:

```javascript
x = 3 * (1 + 2);
```

for a suitably restricted case where semantics are guaranteed equivalent.

Or:

```javascript
const x = foo();
x;
```

versus a transformation preserving the observable semantics.

Then:

```text
run(program A)
run(program B)
```

and expect equivalence.

This can expose optimizer and VM bugs even when you don't have a hand-written expected answer.

---

# 11. And this becomes incredibly powerful for your JIT

Imagine:

```javascript
function f(x) {
    return x + 1;
}

for (...) {
    f(i);
}
```

You can execute it in:

```text
Interpreter
Baseline JIT
Mid-tier JIT
Top-tier JIT
```

and collect:

```text
return values
exceptions
observable object mutations
```

The invariant becomes:

```text
              same semantics
                   ↓
     ┌─────────────┼─────────────┐
     │             │             │
Interpreter   Baseline JIT   Optimizing JIT
```

This means an optimizer can be aggressively rewritten without becoming a second independent implementation of JavaScript semantics.

Its job becomes:

> **produce faster execution of already-defined semantics.**

That greatly reduces the nightmare you were worried about.

---

# 12. Use differential testing specifically around deoptimization

When you eventually add speculative optimization, test this pattern obsessively:

```text
run interpreted
       ↓
collect feedback
       ↓
compile optimized version
       ↓
change assumption
       ↓
deoptimize
       ↓
continue execution
```

For example:

```javascript
function f(x) {
  return x.a + 1;
}

f({ a: 1 });
f({ a: 2 });
f({ a: 3 });

// JIT assumes a particular object shape

f({
  a: "hello",
});
```

The hard bug isn't necessarily:

```text
wrong optimized result
```

It may be:

```text
wrong interpreter state reconstructed during deoptimization
```

Those bugs are exactly why the interpreter should remain available as the semantic fallback.

---

# 13. Rust helps enormously, but Rust does NOT prove Boa is correct

This distinction matters.

Rust gives you strong protection against many classes of memory errors.

But Boa contains unsafe code and eventually will have:

- JIT-generated machine code,
- FFI,
- executable memory,
- custom GC machinery,
- possibly platform-specific code.

The Rust Reference explicitly states that `unsafe` transfers the safety responsibility to the programmer and that unsafe code must uphold its own invariants. ([Rust Documentation](https://dev-doc.rust-lang.org/stable/reference/behavior-considered-undefined.html?utm_source=chatgpt.com))

And some kinds of correctness aren't memory safety at all:

```text
correct memory
+
wrong ECMAScript semantics
```

is still a broken browser.

---

# 14. Audit every `unsafe` boundary separately

I would create a rule:

> **Every unsafe block in the engine must have an explicit safety argument.**

Something like:

```rust
unsafe {
    // SAFETY:
    // ptr was allocated by X
    // alignment is guaranteed by Y
    // lifetime is bounded by Z
}
```

Then maintain a machine-readable inventory:

```text
unsafe block
↓
reason
↓
invariant
↓
who verified it
↓
tests
↓
formal check, if available
```

This is particularly important when you start the JIT.

---

# 15. Run Miri on the testable unsafe core

Miri is useful for detecting classes of Rust undefined behavior such as:

- out-of-bounds accesses,
- use-after-free,
- invalid values,
- alignment problems,
- some data races,
- certain aliasing violations.

But Miri explicitly warns that passing Miri does **not prove the program sound**, and it explores only particular executions. ([GitHub](https://github.com/rust-lang/miri/?utm_source=chatgpt.com))

So:

```text
Miri passes
```

means:

> "Miri did not find a problem in these tested executions."

It does **not** mean:

> "There are no bugs."

---

# 16. Use model checking on the small pieces where it actually makes sense

A tool such as **Kani** can prove bounded properties of Rust code and can find things like:

- panics,
- arithmetic overflow,
- violations of assertions,
- some undefined behavior.

Kani itself explains that it can prove a property, produce a counterexample, or exhaust its resources. ([GitHub](https://github.com/model-checking/kani/blob/main/docs/src/getting-started.md?utm_source=chatgpt.com))

But you don't attempt:

```text
prove all of Boa
```

You prove **critical kernels**.

For example:

```text
Value representation
    ↓
prove tag decoding invariant

GC
    ↓
prove root registration invariant

Bytecode verifier
    ↓
prove invalid bytecode cannot reach certain VM states

JIT
    ↓
prove particular lowering preserves an operation's semantics

Deoptimizer
    ↓
prove frame reconstruction for defined state transitions
```

That is dramatically more realistic.

---

# 17. Concurrency needs its own testing universe

Once you introduce concurrent:

```text
GC
JIT compiler
background compilation
worker threads
network runtime
```

you get bugs that ordinary tests may almost never reproduce.

For small concurrent components, tools such as Loom systematically explore different thread interleavings rather than relying on random timing. ([Docs.rs](https://docs.rs/loom/latest/loom/?search=model&utm_source=chatgpt.com))

Again:

```text
Loom ≠ proof of entire engine
```

but:

```text
Loom + invariants
```

can provide extremely strong assurance for compact concurrent algorithms.

---

# 18. Sanitizers become essential once you have native/JIT/FFI components

When you introduce:

```text
Cranelift
FFI
native code
custom allocators
JIT-generated code
```

you should run heavily instrumented builds.

AddressSanitizer detects memory errors including out-of-bounds access, use-after-free, double-free and related classes. UBSan detects numerous forms of undefined behavior. ([Clang](https://clang.llvm.org/docs/AddressSanitizer.html?utm_source=chatgpt.com))

For a browser engine, I would have dedicated CI configurations such as:

```text
Debug
Release
ASan
UBSan
Miri
fuzz
coverage
stress-GC
```

and run different subsets continuously.

---

# 19. Coverage must be much more sophisticated than "80% lines"

This is another trap.

Imagine:

```text
95% line coverage
```

but nobody has tested:

```text
Proxy + eval + getter + exception + GC + JIT
```

That remaining 5% could contain catastrophic bugs.

So define multiple coverage dimensions.

### Code coverage

```text
line
branch
function
```

### VM coverage

```text
every opcode
every operand form
every exception path
every branch type
```

### semantic coverage

```text
prototype mutations
Proxy traps
accessor properties
private fields
realms
modules
async jobs
WeakRef
FinalizationRegistry
```

### optimization coverage

```text
interpreter → baseline
baseline → optimized
optimized → deopt
optimized → GC
optimized → exception
```

### state-transition coverage

```text
object shape A → B
GC before call
GC during call
exception through optimized frame
```

This is much closer to what you need.

---

# 20. Mutation testing is an underrated technique here

Here's a clever way to answer:

> "Are my tests actually strong enough?"

Intentionally introduce bugs.

For example:

```text
change `<` to `<=`
remove a GC barrier
alter array index calculation
change a bytecode operand
skip a prototype lookup
```

Then run the entire test suite.

If the test suite doesn't detect the injected defect:

```text
test suite weakness discovered
```

That is enormously useful.

You can do this systematically with mutation-testing tooling and custom compiler/runtime transformations.

---

# 21. You need independent implementations of critical algorithms

Do not use one implementation everywhere.

For example, suppose you're implementing:

```text
Array.prototype.sort
```

Having:

```text
implementation
        +
tests generated from implementation
```

can produce circular confidence.

A stronger approach is:

```text
ECMAScript spec
      │
      ├── Test262
      ├── Boa implementation
      └── independent reference implementation/test oracle
```

The same principle applies to:

- parsing
- property lookup
- numeric conversions
- RegExp
- date/time
- typed arrays
- iterator semantics
- Promise machinery.

---

# 22. You also need security fuzzing, not merely correctness fuzzing

A browser is different from a normal application because malicious code is specifically trying to break it.

The attacker controls:

```text
HTML
CSS
JavaScript
SVG
images
fonts
WebAssembly
network responses
file inputs
```

So your threat model is:

> Assume every parser receives adversarial input forever.

That means fuzz:

```text
HTML parser
CSS parser
JS parser
URL parser
image decoders
font parser
media parser
PDF parser
Wasm parser
IPC messages
serialization formats
```

Not merely JavaScript.

---

# 23. You should expect fuzzing to find bugs for the entire lifetime of the browser

This is an important psychological shift.

Google's Fuzzilli documentation exists precisely because dynamic-language engines have enormous hidden state spaces and require continuous generation/mutation strategies. ([GitHub](https://github.com/googleprojectzero/fuzzilli/blob/main/Docs/HowFuzzilliWorks.md?utm_source=chatgpt.com))

The Rust fuzzing ecosystem provides `cargo-fuzz`/libFuzzer, including corpus minimization and coverage generation. ([GitHub](https://github.com/rust-fuzz/cargo-fuzz?utm_source=chatgpt.com))

The mature-browser mindset is:

```text
fuzzing
   ↓
bug
   ↓
minimize
   ↓
fix
   ↓
regression test
   ↓
return bug to corpus
   ↓
fuzz again
```

not:

```text
fuzz for 6 months
↓
declare engine bug-free
```

---

# 24. And here's how I would handle Boa specifically

I would actually change my earlier roadmap from:

```text
Fix all Boa bugs
↓
Now interpreter is correct
↓
Build JIT
```

to:

```text
Audit Boa
↓
Build assurance infrastructure
↓
Fix high-confidence semantic defects
↓
Run continuous differential fuzzing
↓
Increase conformance
↓
Establish semantic invariants
↓
Start baseline JIT
↓
Use interpreter as differential oracle
↓
Continue fixing interpreter and JIT independently
```

That is much safer.

---

# 25. Define a formal "JIT readiness gate"

I would not say:

> "The interpreter is bug-free."

Instead, define something like this:

### Gate A - Conformance

```text
All applicable Test262 tests pass
AND
every excluded test is explicitly justified
AND
no unexplained failure exists
```

Test262 itself provides the large standards-based foundation, while acknowledging that it is not mathematically complete. ([GitHub](https://github.com/tc39/test262?utm_source=chatgpt.com))

### Gate B - Differential

```text
Boa ≈ V8
Boa ≈ SpiderMonkey
Boa ≈ JSC
```

across a very large generated corpus, with discrepancies individually triaged against the ECMAScript specification.

### Gate C - Fuzzing

```text
large continuous fuzz corpus
+
parser fuzzing
+
semantic fuzzing
+
VM fuzzing
+
GC stress
```

### Gate D - Runtime safety

```text
no known UB
no unexplained panic
no sanitizer failure
no unexplained data race
```

### Gate E - Invariants

Critical internal invariants formally specified and tested/proven where practical:

```text
Value representation
GC reachability
object shapes
bytecode validity
environment lifetime
exception propagation
```

### Gate F - Regression

Every bug discovered in the past remains permanently tested.

### Gate G - Independent review

At least some critical components reviewed by people who didn't write them.

Then:

> **Baseline JIT may begin.**

Not because the interpreter has been proven perfect, but because you have built a **high-confidence semantic anchor**.

---

# 26. Then there is an even stronger trick for the JIT

This is probably the most important part of your entire strategy.

Make the optimizer **fail-safe**.

Suppose the JIT isn't sure:

```text
Can this optimization be applied?
```

Instead of:

```text
guess → optimize
```

do:

```text
uncertain
   ↓
deoptimize / interpreter fallback
```

For example:

```text
optimized assumption
       │
       ├── valid → execute fast code
       │
       └── invalid → reconstruct state
                         ↓
                     interpreter
```

This architecture means:

> **A bug in an optimization should ideally produce a deoptimization or performance loss, not silent semantic corruption.**

You won't always achieve this perfectly, but it should be the design philosophy.

---

# 27. You don't actually need to eliminate all interpreter bugs before implementing the JIT

This is the subtle answer to your concern.

Suppose:

```text
Interpreter:
  bug #1843

Baseline JIT:
  perfectly matches interpreter
```

and later:

```text
Differential fuzzer:
  Interpreter ≠ V8
  Interpreter ≠ SpiderMonkey
  Interpreter ≠ spec
```

You fix the interpreter.

Then the JIT's differential suite tells you whether the optimized tier still agrees with the corrected semantics.

The architecture becomes:

```text
             external oracle
             /      |      \
            /       |       \
        V8       Spec      JSC
          \        |       /
           \       |      /
             Boa interpreter
                    │
           semantic reference
                    │
         ┌──────────┼──────────┐
         ↓          ↓          ↓
      Baseline    Mid-tier   Top-tier
         JIT        JIT        JIT
```

So later tiers aren't built on a frozen, sacred implementation.

They're continuously compared against the current semantic reference.

---

# 28. What you can actually guarantee

This is how I'd phrase the assurance levels.

| Statement                                                     | Can you establish it                                                          |
| ------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| All currently known Boa bugs fixed                            | **Yes, within your defined bug database**                                     |
| All applicable Test262 tests pass                             | **Yes**                                                                       |
| No known sanitizer violations in tested configurations        | **Yes**                                                                       |
| No known panic paths in tested scope                          | **Yes**                                                                       |
| Critical invariants formally proven                           | **Yes, selectively**                                                          |
| JIT agrees with interpreter over huge corpus                  | **Yes**                                                                       |
| Boa agrees with multiple independent engines over huge corpus | **Yes**                                                                       |
| Very high confidence in correctness                           | **Yes**                                                                       |
| No bugs exist anywhere in the code                            | **No**                                                                        |
| No future bug can exist                                       | **No**                                                                        |
| Entire browser formally proven correct                        | **Technically conceivable in principle, wildly impractical for this project** |

That last distinction is crucial.

---

# 29. There is one more thing I would change in your project philosophy

Don't think of:

```text
"correct interpreter"
```

as a **state you reach once**.

Think of it as:

```text
                     ┌────────────┐
                     │ Specification│
                     └──────┬─────┘
                            ↓
                    External tests
                            ↓
                 ┌──────────┴──────────┐
                 │                     │
         Boa interpreter          Other engines
                 │                     │
                 └──────────┬──────────┘
                            ↓
                       Fuzzing
                            ↓
                         Bugs
                            ↓
                         Fixes
                            ↓
                     Regression tests
                            ↓
                     Better interpreter
                            ↓
                      Better JITs
                            ↺
```

**Correctness becomes a continuous process rather than a milestone.**

And that is much closer to how mature browser engines actually operate. WebKit, for example, requires regression suites before changes land, and the web-platform-tests project exists specifically to provide cross-browser behavioral testing; its stated purpose is to give browser projects confidence that implementations remain interoperable. ([WebKit](https://webkit.org/regression-testing/?utm_source=chatgpt.com))

---

# 30. So, for your project, I would use this exact order

I would now revise the beginning of the roadmap to:

```text
PHASE 0
Pin Boa version
       ↓
Inventory architecture / dependencies / unsafe code
       ↓
Identify semantic choke points
       ↓
Build CI + regression infrastructure

PHASE 1
Test262
+
Boa tests
+
spec-driven tests
+
parser fuzzing

PHASE 2
Differential testing
Boa ↔ V8
Boa ↔ SpiderMonkey
Boa ↔ JSC
       ↓
massive generated corpus

PHASE 3
Coverage-guided fuzzing
+
Fuzzilli
+
VM-bytecode fuzzing
+
GC stress
+
metamorphic testing

PHASE 4
Fix discovered semantic/runtime bugs
       ↓
Every bug becomes a permanent regression test

PHASE 5
Formalize internal invariants
       ↓
Miri / sanitizers / model checking
       ↓
formal proofs for critical kernels

PHASE 6
Interpreter becomes
"high-confidence semantic reference"
       ↓
NOT "proven bug-free"

PHASE 7
Baseline JIT
       ↓
differentially checked against interpreter

PHASE 8
Deoptimization + feedback
       ↓
optimized JIT

PHASE 9
Continue all testing permanently
```

That is substantially safer than attempting to **“find every bug in Boa first.”**

The most important conceptual change is this:

> **You don't solve the problem by proving Boa has no bugs before proceeding. You solve it by designing the entire engine so that bugs are continuously discoverable, independently detectable, permanently regression-tested, and prevented from propagating silently into later execution tiers.**

And your intuition about **unknown, potentially critical bugs** is absolutely correct. Boa's current issue tracker can tell us what people have found; it cannot tell us what nobody has found yet. Even the official Test262 project explicitly cautions that its coverage is not complete. ([GitHub](https://github.com/boa-dev/boa/issues))

For a project as ambitious as yours, I would consider **building this verification/differential/fuzzing infrastructure almost as important as building the JIT itself**. Without it, a sophisticated JIT becomes a giant multiplier of uncertainty. With it, the JIT becomes another independently testable layer sitting above a continuously checked semantic core.

---

**Sources:**

- [GitHub - boa-dev/boa: Boa is an embeddable Javascript engine written in Rust. · GitHub](https://github.com/boa-dev/boa)
- [Behavior considered undefined - The Rust Reference](https://dev-doc.rust-lang.org/stable/reference/behavior-considered-undefined.html?utm_source=chatgpt.com)
- [loom - Rust](https://docs.rs/loom/latest/loom/?search=model&utm_source=chatgpt.com)
- [AddressSanitizer - Clang](https://clang.llvm.org/docs/AddressSanitizer.html?utm_source=chatgpt.com)
- [Regression Testing | WebKit](https://webkit.org/regression-testing/?utm_source=chatgpt.com)

---

Powered by [ChatGPT Exporter](https://www.chatgptexporter.com)
