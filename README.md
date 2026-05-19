# vtube-studio-rs

A Rust-first macOS prototype for a VTube Studio-like avatar host.

The first milestone is intentionally narrow: keep the avatar window and frame
loop alive when switching macOS Desktops/Spaces. Full Live2D loading, tracking,
plugin APIs, and scene editing can sit on top after this behavior is proven.

## Current Prototype

- Creates a native AppKit borderless floating window from Rust.
- Sets macOS Space behavior with `canJoinAllSpaces`, `stationary`, and
  `fullScreenAuxiliary`.
- Runs a manual 60 FPS frame loop instead of depending on a foreground-only UI
  callback.
- Requests an `NSProcessInfo` activity token to reduce App Nap and automatic
  termination while the avatar renderer is active.
- Shows an in-window diagnostics overlay with FPS, frame timing, slow-frame
  count, total frames, uptime, and App Nap guard status.
- Logs frame gaps over 250 ms to the terminal, which helps catch Space switches
  or OS throttling events during manual testing.
- Loads the Live2D Cubism model manifest from `public/model/0.model3.json`,
  validates the referenced `.moc3`, textures, physics, and display-info files,
  and uses the primary texture atlas as the current placeholder avatar content.
- With `cubism-core` enabled, renders a first proof-of-life frame through a CPU
  software rasterizer using Cubism drawable positions, UVs, indices, render
  order, opacity, and texture atlases.
- With `metal-renderer` enabled, attaches a `CAMetalLayer` to the avatar window
  and draws Cubism drawables through a Metal pipeline.
- Draws a small animated Core Animation placeholder layer as the first avatar
  stand-in when Cubism Core is not enabled.

## Run

```bash
cargo run
```

Use `Ctrl-C` in the terminal to stop the prototype.

The current default model path is:

```text
public/model/0.model3.json
```

`public/` is intentionally treated as local-only content and is not expected to
be uploaded to GitHub. Put your own Live2D Cubism model files there when running
the app locally. The expected default layout is:

```text
public/
  model/
    0.model3.json
    0.moc3
    0.physics3.json
    0.cdi3.json
    0.8192/
      texture_00.png
      texture_01.png
```

You can pass a different Cubism manifest path as the first argument:

```bash
cargo run -- public/model/0.model3.json
```

By default, the app validates the Cubism files and displays the first texture
atlas as a placeholder. With `cubism-core`, it evaluates `.moc3` drawables
through Live2D Cubism Core. With `metal-renderer`, it also draws those drawable
meshes through Metal.

This is still not a complete VTube Studio replacement. The current renderer has
the resource entry point, Cubism Core model evaluation, texture atlas upload,
and basic drawable mesh rendering. The next layer is proper Live2D rendering
behavior: masks, additive/multiply blend modes, physics-driven parameters,
motion/expression playback, and tracking input.

## Live2D Cubism Core

The project supports an optional Cubism Core integration through
`live2d-cubism-core-sys`. The official Cubism Core is not committed to this
repository; download Cubism SDK for Native from Live2D and keep it outside
version control.

Expected SDK root layout:

```text
CubismSdkForNative-*/
  Core/
    include/
      Live2DCubismCore.h
    lib/
      macos/
        libLive2DCubismCore.a
```

Run with Cubism Core enabled:

```bash
CUBISM_CORE_LIB_DIR="$PWD/public/CubismSdkForNative/Core/lib/macos/arm64" \
CUBISM_CORE_INCLUDE_DIR="$PWD/public/CubismSdkForNative/Core/include" \
  cargo run --features cubism-core -- public/model/0.model3.json
```

You can also point to the full SDK root. Use an absolute path; direct lib/include
paths are more reliable for local app runs:

```bash
LIVE2D_CUBISM_SDK_NATIVE_DIR="$PWD/public/CubismSdkForNative" \
  cargo run --features cubism-core -- public/model/0.model3.json
```

With `cubism-core` enabled, the app loads the `.moc3`, validates consistency,
initializes a `csmModel`, calls `csmUpdateModel` each frame, and reports Core
version, moc version, parameter count, part count, drawable count, and canvas
info in the diagnostics overlay. The Rust wrapper also exposes parameter and
drawable metadata plus drawable frame buffers.

The `metal-renderer` feature replaces the CPU proof-of-life display path with a
real `CAMetalLayer`. It uploads the texture atlases to Metal textures, compiles
shader pipelines, sorts drawables by render order, and draws indexed triangle
meshes from Cubism's updated vertex/UV/index buffers. Normal, additive, and
multiplicative drawable blend modes now use separate Metal pipeline states.
Masked drawables render their Cubism clipping masks into tiles in a reusable
offscreen Metal mask atlas before the main pass samples the relevant tile.
Dynamic drawable vertices are written into a small ring of shared Metal buffers
each frame, while drawable index buffers are cached and reused by both the mask
pass and the main pass.

```bash
CUBISM_CORE_LIB_DIR="$PWD/public/CubismSdkForNative/Core/lib/macos/arm64" \
CUBISM_CORE_INCLUDE_DIR="$PWD/public/CubismSdkForNative/Core/include" \
  cargo run --features metal-renderer -- public/model/0.model3.json
```

The `.gitignore` excludes `public/` and `CubismSdkForNative*/` so model files
and a locally downloaded SDK are not accidentally committed.

Reference: Live2D's Core API documentation describes Cubism Core as a C API for
handling `.moc3` models. It calculates model data such as vertex information;
drawing remains the application's responsibility.

Pure Rust parser crates such as `live2d-parser` can help read model metadata,
resource paths, and some Cubism v3 structures, but a parser is not a complete
runtime. It does not replace Cubism Core's model calculation responsibilities:
deformers, parameter evaluation, drawable updates, vertex calculation, opacity,
masking, render order, and compatibility across `.moc3` versions still need a
runtime layer.

## Why This Solves The First Problem

VTube-style apps often appear to freeze on macOS when their render/update work
is tied too closely to normal window focus, normal Space membership, or UI
callbacks that stop firing when the app is no longer active in the current
Desktop.

This prototype separates the first critical pieces:

- The avatar window is allowed to join every Space.
- The window stays floating and auxiliary to full-screen Spaces.
- The frame loop is owned by the process, not only by a visible focused window.
- The process asks macOS not to treat the active avatar renderer like an idle
  background app.
- The diagnostics overlay makes Space-switch freezes visible immediately: FPS
  and frame count should keep advancing, while slow-frame count and max frame
  delta expose smaller stalls.

## Manual Test

1. Run `cargo run`.
2. Move the avatar window where you can see it.
3. Switch between macOS Desktops/Spaces several times.
4. Watch the diagnostics overlay:
   - `Frames` should keep increasing.
   - `FPS` should recover to roughly 60.
   - `Frame delta max` and `Slow frames` show how harsh the transition was.
5. Check the terminal for `Long frame gap` lines. Gaps over 250 ms are worth
   investigating.

## Next Milestones

1. Batch compatible drawables and reduce pipeline/texture state churn.
2. Drive parameters from physics, motions, expressions, and tracking input.
3. Add motion/expression loading and a first idle animation driver.
4. Add explicit active Space transition detection and structured trace output.
5. Implement the VTube Studio plugin WebSocket API compatibility layer.
