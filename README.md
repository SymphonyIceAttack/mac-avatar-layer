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

For now, the app validates the Cubism files and displays the first texture
atlas as a placeholder. Rendering the real `.moc3` mesh/deformer data requires
the Live2D Cubism runtime, which is the next integration layer.

In other words, the current implementation has connected the model resource
entry point and can display the texture atlas, but it is not yet a complete
Live2D `.moc3` mesh renderer. Making the avatar actually move requires
integrating the Live2D Cubism Native runtime, then driving the model with
`.moc3`, texture atlases, physics, and parameter updates.

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
drawable metadata plus drawable frame buffers for a future renderer. Actual
mesh drawing now has a CPU proof-of-life path, but this is not the final
renderer. The app still needs a Metal or wgpu renderer for correct high
performance rendering, masks, blend modes, and real-time use.

The `metal-renderer` feature adds the first Metal backend skeleton. It creates
a Metal device and command queue, then probes Cubism drawable and triangle
counts against the loaded model. It does not yet replace the CPU rasterizer or
attach a `CAMetalLayer`.

```bash
CUBISM_CORE_LIB_DIR="$PWD/public/CubismSdkForNative/Core/lib/macos/arm64" \
CUBISM_CORE_INCLUDE_DIR="$PWD/public/CubismSdkForNative/Core/include" \
  cargo run --features metal-renderer -- public/model/0.model3.json
```

The `.gitignore` excludes `CubismSdkForNative*/` so a locally downloaded SDK is
not accidentally committed.

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

1. Attach a `CAMetalLayer` to the avatar window and render Cubism drawables
   through Metal.
2. Add mask buffers, blend modes, texture upload, and high-performance drawable
   batching.
3. Add explicit active Space transition detection and structured trace output.
4. Add camera/face-tracking input as a separate thread with timestamped frames.
5. Implement the VTube Studio plugin WebSocket API compatibility layer.
