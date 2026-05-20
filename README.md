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
  multiply/screen colors, double-sided culling flags, optional texture-atlas
  mipmaps/anisotropic sampling, configurable drawable/part hiding, and bucketed
  Retina mask texture sizing for resize stability.
- Synchronizes the Metal layer frame, `contentsScale`, drawable size, mask
  textures, offscreen textures, and MSAA texture against the current AppKit
  window bounds/backing scale before each render.
- Applies Cubism-style `ppu / physicalMaskWidth` and
  `ppu / physicalMaskHeight` clipping precision branches when canvas ppu is
  available.
- Supports optional high precision masks where each clipping context owns a
  full-size mask texture and is redrawn immediately before each masked drawable
  instead of sharing an RGBA atlas tile.
- Supports multiple shared mask render textures when the clipping context count
  exceeds one texture's practical capacity.
- Detects Cubism Core offscreen drawable counts and uses a first-pass Metal
  offscreen render/composite path for models that need it.
- Decodes Cubism v5 extended blend modes and uses a first-pass Metal extended
  blend shader with render-target snapshots.
- Uses layer edge antialiasing and 4x MSAA when supported to smooth transparent
  window and avatar mesh edges.
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

Probe all local models without opening a window:

```bash
./scripts/probe-risk-models.sh
./scripts/sample-compatibility-sweep.sh

CUBISM_CORE_LIB_DIR="$PWD/public/CubismSdkForNative/Core/lib/macos/arm64" \
CUBISM_CORE_INCLUDE_DIR="$PWD/public/CubismSdkForNative/Core/include" \
  cargo run --features cubism-core -- --probe-models public
```

This scans every `.model3.json`, initializes Cubism Core, and prints parameter,
part, drawable, masked drawable, maximum mask, blend-mode, inverted-mask, and
offscreen counts. It also labels each model `risk:low`, `risk:medium`, or
`risk:high` for renderer compatibility triage. High-risk models print the
specific reason, such as dense clipping, many masked drawables, offscreen
objects, extended blends, masked extended drawables, extended offscreens,
masked offscreens, or inverted masks. The script writes
`target/render-regression/probe.txt`, and the render regression report embeds
that probe output.
`sample-compatibility-sweep.sh` scans the official SDK sample resources and
writes `target/render-regression/compatibility-sweep.md`, which ranks models by
risk shape and recommends whether the screenshot matrix needs another stress
model beyond `Mao` and `Ren`.

You can also set `LIVE2D_CUBISM_SDK_NATIVE_DIR` to point at a different SDK root.

Capture a cropped Metal renderer screenshot for visual regression checks:

```bash
./scripts/capture-metal.sh public/CubismSdkForNative/Samples/Resources/Mao/Mao.model3.json
./scripts/capture-metal.sh public/CubismSdkForNative/Samples/Resources/Ren/Ren.model3.json
./scripts/capture-risk-models.sh
./scripts/capture-mask-matrix.sh
./scripts/capture-offscreen-matrix.sh
./scripts/ren-offscreen-audit.sh
./scripts/capture-rice-candidate.sh
./scripts/capture-rice-stress.sh
./scripts/capture-quality-matrix.sh
./scripts/capture-full-matrix.sh
```

Screenshots are written to `target/render-regression/`. The script reuses the
same SDK auto-detection as `run-metal.sh`, waits for the app window, captures
only that window, and closes the launched process. `capture-risk-models.sh`
captures the local model plus the SDK `Mao` and `Ren` stress models, preserving
timestamped screenshots and refreshing `latest-*.png` copies for quick visual
comparison after renderer changes. Set `WAIT_SECONDS` if a machine needs longer
for the first build/startup, or `POST_WINDOW_WAIT_SECONDS` if the screenshot
should wait longer for diagnostics and motion to settle after the window appears.
`capture-mask-matrix.sh` temporarily switches the local renderer config and
captures the `Mao` stress model in shared-mask, high-precision-mask, and
no-mask modes under `target/render-regression/mask-matrix/`, then restores the
previous `vtube-studio-rs.toml`.
`capture-offscreen-matrix.sh` does the same for the `Ren` offscreen/extended
blend stress model under `target/render-regression/offscreen-matrix/`.
`ren-offscreen-audit.sh` writes
`target/render-regression/ren-offscreen-audit.md`, a focused report for Ren's
nested offscreen, masked offscreen, extended offscreen, and extended drawable
distribution before changing the offscreen compositor.
`capture-rice-stress.sh` captures `Rice` in shared, high-precision, and
no-mask modes under `target/render-regression/rice-candidate/` when the SDK
sample is available. Rice is an optional stress model in the full matrix: it
covers additive, inverted-mask, and translucent-drawable risks, and is skipped
automatically when the local SDK sample is missing. `capture-rice-candidate.sh`
remains as a compatibility alias for the earlier candidate workflow.
Offscreen models currently fall back from high-precision masks to shared masks;
the overlay marks this as `mask shared(offscreen)`.
The corresponding `high_precision_mask_fallback` renderer event includes
offscreen, masked offscreen, extended offscreen, masked extended drawable,
nested offscreen, and maximum offscreen depth counts.
`capture-quality-matrix.sh` captures the default model plus `Mao` and `Ren`
with texture atlas mipmaps off/on under `target/render-regression/quality-matrix/`
so texture shimmer and atlas island bleed can be compared.
Each capture script refreshes `target/render-regression/report.md` through a
bounded report wrapper. Set `VTUBE_RS_SKIP_REPORT=1` when chaining several
capture scripts and generate the report once at the end.
`capture-full-matrix.sh` is the preferred complete visual sweep: it cleans
generated render artifacts once, runs the risk, mask, offscreen, optional Rice
stress, and quality matrices with report generation skipped inside each step,
performs process cleanup between steps, and then writes one final Markdown
report.
The report is a Markdown index with latest screenshot paths, manual review
checklist, embedded thumbnail previews/contact sheet, model risk probe output,
automatic review focus, renderer fallback events, and recent renderer events
from capture logs.
Generated test artifacts under `target/render-regression/` and
`target/space-test/` are cleaned automatically before these scripts run. Set
`VTUBE_RS_SKIP_TARGET_CLEAN=1` to keep previous local artifacts for comparison.
Run `./scripts/clean-target.sh --all` when you also want to remove Cargo build
outputs.

Metal renderer lifecycle logs use `renderer_event=...` records so Space-switch
and sleep/wake testing can be checked from the terminal. Useful events include
`instance_guard_acquired`, `app_nap_guard_started`, `window_configured`,
`app_active_changed`, `window_visible_changed`, `window_occlusion_changed`,
`metal_initialized`, `contents_scale_changed`, `drawable_size_changed`,
`mask_tile_size_changed`,
`mask_atlas_resized`, `offscreen_texture_size_changed`,
`next_drawable_unavailable`, `next_drawable_recovered`, `long_frame_gap`, and
`display_wake_inferred`.
The app prevents duplicate local avatar instances with
`target/vtube-studio-rs.pid`; set `VTUBE_RS_ALLOW_DUPLICATE_INSTANCE=1` only
when intentionally debugging multiple windows.
`scripts/run-space-test.sh` writes machine logs to `target/space-test/*.log`
and a Markdown checklist/report to `target/space-test/*.md`.

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
high_precision_masks = false
atlas_mipmaps = false
debug_texture_mode = "none"
hidden_drawables = []
hidden_parts = []
only_drawables = []
only_parts = []
highlight_drawables = []
highlight_parts = []

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

1. Run `./scripts/run-space-test.sh`.
2. Move the avatar window where it remains visible.
3. Switch between macOS Desktops/Spaces several times.
4. Watch the diagnostics overlay:
   - `Frames` should keep increasing.
   - `FPS` should recover to roughly 60.
   - `Frame delta max` and `Slow frames` show transition stalls.
5. Confirm startup logs include `renderer_event=app_nap_guard_started` and
   `renderer_event=window_configured`.
6. Check the terminal for `renderer_event=long_frame_gap` lines and any
   `renderer_event=next_drawable_unavailable` / `next_drawable_recovered`
   pairs.
7. After display sleep/wake, check whether `renderer_event=display_wake_inferred`
   appears and whether FPS/Frames recover afterward.
8. Press `Ctrl-C` in the terminal. The script prints an event summary and saves
   the full log plus a Markdown report under `target/space-test/`.
9. Open the generated `space-test-*.md` report and fill in the manual checklist.
   Use the automatic assessment table to decide whether the next fix should
   focus on CAMetalLayer recovery, window behavior, or normal transition stalls.

Current baseline: `target/space-test/space-test-20260520-191559.md` passed
startup guard, drawable recovery, and display wake automatic checks. It logged
two long-frame gaps as Space transition signals and no drawable loss.

## Next Milestones

1. Keep README and PRD aligned as capabilities change.
2. Improve renderer quality: validate mipmaps/anisotropic filtering, refine
   Retina-stable mask texture sizing, and transparent edge antialiasing.
3. Validate Cubism clipping parity against more official sample models.
4. Continue macOS Space/display reliability passes as new failure cases appear.
5. Later, investigate webcam/ARKit tracking and VTube Studio plugin API
   compatibility.
