# vtube-studio-rs

A Rust-first macOS prototype for a VTube Studio-like avatar host.

The first product goal is reliability on macOS: keep the avatar window and
render loop alive across Desktop/Space switches, full-screen apps, and ordinary
focus changes. The app now loads local Live2D Cubism assets, evaluates `.moc3`
through Cubism Core, renders meshes with Metal, and drives parameters from idle
motion, expressions, physics, mouse input, and microphone volume.

## Current Prototype

- Creates a native AppKit borderless floating window from Rust.
- Sets macOS Space behavior with `canJoinAllSpaces`, `stationary`, and
  `fullScreenAuxiliary`.
- Runs a manual 60 FPS frame loop and logs long frame gaps over 250 ms.
- Requests an `NSProcessInfo` activity token to reduce App Nap while rendering.
- Loads `model3.json` from local `public/` assets and validates referenced
  `.moc3`, textures, physics, display info, motions, and expressions.
- Integrates Cubism Core through `live2d-cubism-core-sys`.
- Renders Cubism drawable meshes through Metal with texture atlases, render
  order, normal/additive/multiplicative blending, clipping masks, RGBA mask
  channels, 1/2/4/9 mask layout, explicit mask/draw matrices, per-drawable
  multiply/screen colors, double-sided culling flags, mipmapped texture
  atlases, and anisotropic atlas sampling.
- Drives parameters through a single `MotionController` update order:
  blink/breath -> idle motion -> expression -> mouse/mic input -> physics ->
  config overrides -> `csmUpdateModel`.
- Supports `.motion3.json` idle playback, `.exp3.json` add/multiply/overwrite,
  `.physics3.json` lightweight particle simulation, mouse head/eye tracking,
  and microphone RMS mouth opening.

## Local Assets

`public/` is intentionally local-only and ignored by Git. Do not commit official
Live2D SDK files or model assets.

Default model layout:

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

Default SDK layout:

```text
public/
  CubismSdkForNative/
    Core/
      include/
        Live2DCubismCore.h
      lib/
        macos/
          arm64/
            libLive2DCubismCore.a
          x86_64/
            libLive2DCubismCore.a
```

## Run

The recommended local command is:

```bash
./scripts/run-metal.sh
```

It defaults to `public/model/0.model3.json`, auto-detects
`public/CubismSdkForNative`, sets `CUBISM_CORE_LIB_DIR` and
`CUBISM_CORE_INCLUDE_DIR`, and closes old `target/debug/vtube-studio-rs`
instances before launching. Pass a different model path as the first argument:

```bash
./scripts/run-metal.sh public/CubismSdkForNative/Samples/Resources/Rice/Rice.model3.json
```

To keep old instances alive during development:

```bash
RUN_METAL_KILL_OLD=0 ./scripts/run-metal.sh
```

Manual equivalent:

```bash
CUBISM_CORE_LIB_DIR="$PWD/public/CubismSdkForNative/Core/lib/macos/arm64" \
CUBISM_CORE_INCLUDE_DIR="$PWD/public/CubismSdkForNative/Core/include" \
  cargo run --features metal-renderer -- public/model/0.model3.json
```

You can also set `LIVE2D_CUBISM_SDK_NATIVE_DIR` to point at a different SDK root.

## App Configuration

Runtime options are read once at startup from `vtube-studio-rs.toml` in the
project root. The file is local-only and ignored by Git; copy
`vtube-studio-rs.example.toml` when you want to customize a run. If the file is
missing, the app uses the same defaults as the example.

```toml
[diagnostics]
show = true

[renderer]
disable_masks = false

[motion]
# expression = "smile"
blink_interval = 3.8
blink_duration = 0.18

[input.mouse]
enabled = false
smoothing = 10.0

[input.microphone]
enabled = false
gain = 7.0
noise_gate = 0.025
smoothing = 18.0

[overrides]
# mouth_open = 1.0
# mouth_form = 0.0
```

SDK path variables such as `LIVE2D_CUBISM_SDK_NATIVE_DIR`,
`CUBISM_CORE_LIB_DIR`, and `CUBISM_CORE_INCLUDE_DIR` remain build/run-script
inputs because they are needed before the app starts.

## Test

```bash
cargo fmt --check
cargo check
cargo test
CUBISM_CORE_LIB_DIR="$PWD/public/CubismSdkForNative/Core/lib/macos/arm64" \
CUBISM_CORE_INCLUDE_DIR="$PWD/public/CubismSdkForNative/Core/include" \
  cargo test --features metal-renderer
```

## Manual Space Test

1. Run `./scripts/run-metal.sh`.
2. Move the avatar window where it remains visible.
3. Switch between macOS Desktops/Spaces several times.
4. Watch the diagnostics overlay:
   - `Frames` should keep increasing.
   - `FPS` should recover to roughly 60.
   - `Frame delta max` and `Slow frames` show transition stalls.
5. Check the terminal for `Long frame gap` lines.

## Next Milestones

1. Keep README and PRD aligned as capabilities change.
2. Improve renderer quality: mipmaps, anisotropic filtering, Retina-stable mask
   texture sizing, and transparent edge antialiasing.
3. Close more Cubism clipping parity: official precision branches, high
   precision masks, and multi mask render textures.
4. Run a dedicated macOS Space/display reliability pass.
5. Later, investigate webcam/ARKit tracking and VTube Studio plugin API
   compatibility.
