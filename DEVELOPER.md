# MacAvatarLayer Developer Guide

This document contains the implementation notes, advanced workflows, diagnostics, capture paths, configuration reference, and testing guidance for contributors and developers sharing code changes. For the short end-user build/run path, see [README.md](README.md).


![MacAvatarLayer logo](logo/mac-avatar-layer-logo.png)

A Rust-first macOS prototype for a macOS-native Live2D avatar host.

The first product goal is reliability on macOS: keep the avatar window and
render loop alive across Desktop/Space switches, full-screen apps, and ordinary
focus changes. The app now loads local Live2D Cubism assets, evaluates `.moc3`
through Cubism Core, renders meshes with Metal, and drives parameters from idle
motion, expressions, physics, mouse input, and microphone volume.

Live2D's official `Cubism SDK for Native` support matrix documents the sample
renderer implementations shipped by Live2D. It does not ship a macOS Metal
sample renderer, but this project does not depend on that sample renderer. It
uses Cubism Core for model evaluation and owns a Rust/Metal renderer for macOS.

## Current Prototype

- Creates a native AppKit borderless floating window from Rust.
- Uses a non-activating `NSPanel` overlay with configurable CoreGraphics window
  level; the default is `window_level = "screen_saver"`.
- Reads `window_width` and `window_height` from `[app]`; the local configs are
  set to `540x720`, which is 1.5x the original prototype window.
- Applies `canJoinAllSpaces`, `canJoinAllApplications`, `stationary`,
  `ignoresCycle`, and `fullScreenAuxiliary` so the avatar behaves less like a
  normal desktop window during Space transitions.
- Pumps AppKit events in non-blocking default, event-tracking, and modal run
  loop modes so the render loop is less likely to stall during Space gestures.
- Uses a transparent avatar window by default; diagnostics are off in the local
  dev/build config so the visible surface is just the Live2D model.
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

### Quick Start

For another user who only wants to run the local avatar app, the setup path is:

```bash
xcode-select --install
cargo xtask doctor
cargo xtask start
```

`xcode-select --install` is only needed when Xcode Command Line Tools are not
installed yet. `cargo xtask doctor` checks the local config, selected model,
Cubism Core SDK files, and camera/microphone entitlements. `cargo xtask start`
is the short command for the optimized local run path; it is equivalent to
`cargo xtask run-metal --release`.

The project still needs local assets that cannot be committed to the repository:

- Live2D Cubism Native SDK under `public/CubismSdkForNative`, or the
  `LIVE2D_CUBISM_SDK_NATIVE_DIR`, `CUBISM_CORE_INCLUDE_DIR`, and
  `CUBISM_CORE_LIB_DIR` environment variables. `cargo xtask start` does not
  download the SDK automatically; download
  [Live2D Cubism SDK for Native](https://www.live2d.com/en/sdk/about/) from
  Live2D's official SDK page and install it locally first. No project command
  downloads the Live2D SDK; xtask commands only detect the local SDK path and
  print repair instructions when it is missing.
- A selected `.model3.json`, usually `public/model/0.model3.json`.

To choose a different model before starting:

```bash
cargo xtask list-models
cargo xtask select-model --build public/model/0.model3.json
cargo xtask start
```

You can also start one model for a single run without changing the config:

```bash
cargo xtask start public/model/0.model3.json
```

### Tooling Dependencies

The local helper commands are intended for macOS development. Before running
capture, audit, or Space reliability commands, make sure these tools are
available:

- Rust toolchain with `cargo`.
- Xcode Command Line Tools for standard macOS developer utilities.
- Screen Recording permission for the terminal/Codex app may be required on
  the first capture run.
- Development runs enable the ScreenCaptureKit runtime probe by default through
  `[capture.screen_capture_kit].enabled = true`; macOS may ask for Screen
  Recording permission for the local `.app` wrapper before probe frames appear.
  Release/build configs disable this probe by default.
- Microphone permission is required only when `[input.microphone].enabled =
  true`; if startup reports a microphone failure, allow the terminal/Codex app
  under macOS System Settings > Privacy & Security > Microphone, or disable the
  microphone input in the active profile config.
- Window lookup and screenshot capture are implemented in Rust through
  CoreGraphics, so capture commands no longer require `swift` or
  `screencapture`.
- Stale renderer process cleanup is implemented in Rust, so run/capture
  commands no longer require `pkill`.
- Syphon support has been removed. OBS integration now uses the normal
  transparent app window through OBS Window Capture or macOS Screen Capture.

Install missing macOS tools with:

```bash
xcode-select --install
```

The Rust-native audit and visual diff commands do not require ImageMagick.
`public/CubismSdkForNative` and local model assets are still required for
commands that load official sample models such as `Mao`, `Ren`, and `Rice`.

Local tooling is exposed through the Rust `xtask` crate:

```bash
cargo xtask clean --generated
cargo xtask clean --all
cargo xtask build-app
cargo xtask build-app --release
cargo xtask capture-full-matrix
cargo xtask capture-metal public/model/0.model3.json
cargo xtask capture-mask-matrix
cargo xtask capture-offscreen-matrix
cargo xtask capture-quality-matrix
cargo xtask capture-risk-models
cargo xtask capture-rice-stress
cargo xtask configure-obs-virtual-camera --build
cargo xtask configure-obs-virtual-camera --build --quality balanced
cargo xtask configure-obs-virtual-camera --build --offscreen
cargo xtask doctor
cargo xtask fix-wwdr-cert
cargo xtask list-models
cargo xtask mao-mask-audit
cargo xtask probe-risk-models public/model
cargo xtask quality-visual-diff
cargo xtask ren-visual-diff
cargo xtask ren-offscreen-audit
cargo xtask render-regression-report
cargo xtask rice-stress-audit
cargo xtask run-metal
cargo xtask run-metal --release
cargo xtask run-space-test
cargo xtask sample-compatibility-sweep
cargo xtask select-model --dev public/model/0.model3.json
cargo xtask select-model --build public/model/0.model3.json
cargo xtask start
cargo xtask tune-input --build camera expressive
```

`cargo xtask build-app` and `cargo xtask run-metal` do not require an Apple
developer certificate for the normal desktop-window workflow. If a local Apple
codesigning identity exists in Keychain, xtask uses it; otherwise the app is
signed ad-hoc, which is enough for local launching, camera permission prompts,
and OBS/window-capture use.

If contributors intentionally force `MAC_AVATAR_LAYER_CODESIGN_IDENTITY` and
Xcode shows an Apple Development certificate but `security` still reports no
valid codesigning identity, first check the local identity list:

```bash
security find-identity
security find-identity -v -p codesigning
```

When the certificate appears with `CSSMERR_TP_NOT_TRUSTED` and the
codesigning-specific list ends with `0 valid identities found`, the certificate
and private key are present but the Apple WWDR intermediate trust chain is
broken. Run the built-in repair command to install the current Apple Worldwide
Developer Relations G3 intermediate certificate and re-check the codesigning
identity:

```bash
cargo xtask fix-wwdr-cert
```

The expected result is one valid Apple Development identity. If the identity is
still not trusted, open Keychain Access, search for `Apple Worldwide Developer
Relations Certification Authority`, remove the expired 2023 WWDR intermediate
certificate, keep the G3 certificate that expires in 2030, and leave its trust
setting at `Use System Defaults`. The repair command stores the downloaded
certificate under
`target/codesign/AppleWWDRCAG3.cer`, so it remains a generated local artifact
and is not committed.

The recommended local command for normal users is:

```bash
cargo xtask start
```

`start` is equivalent to `cargo xtask run-metal --release`. With no argument it
uses `[model].path` from the build profile config, falling back to
`public/model/0.model3.json` when unset. It auto-detects
`public/CubismSdkForNative`, sets `CUBISM_CORE_LIB_DIR` and
`CUBISM_CORE_INCLUDE_DIR`, and closes old `mac-avatar-layer` instances before
launching through the local `.app` wrapper so camera permissions use the stable
`io.github.symphonyiceattack.mac-avatar-layer` identity.

For development runs against `mac-avatar-layer.dev.toml`, use:

```bash
cargo xtask run-metal
```

To spell out the optimized command directly:

```bash
cargo xtask run-metal --release
```

To build and sign the local `.app` wrapper without launching it:

```bash
cargo xtask build-app
cargo xtask build-app --release
```

This uses Cubism Core SDK auto-detection, the stable `io.github.symphonyiceattack.mac-avatar-layer`
bundle identity, and the same auto-detected local codesigning path as
`run-metal`.
Development and release builds both use the normal transparent window output.
Release builds use the optimized profile in `mac-avatar-layer.build.toml` without
requiring `Syphon.framework`.

`run-metal` always compiles `metal-renderer`, `camera-tracking`, and
`screen-capture-kit`. The active TOML decides whether the camera opens through
`[input.camera].enabled` and whether the ScreenCaptureKit diagnostic probe
starts through `[capture.screen_capture_kit].enabled`.
The local build config enables camera input by default, so
`cargo xtask run-metal --release` is the optimized camera-capable transparent
window run path. App stdout/stderr are written under
`target/camera-test/run-metal-*.log`.

### OBS Window Capture Preset

The supported camera-output path is OBS, not an in-project virtual camera
driver:

```text
MacAvatarLayer -> OBS Window Capture -> OBS Virtual Camera -> Zoom/Discord/Meet
```

This keeps virtual-camera implementation, signing, and camera-app compatibility
inside OBS Studio. MacAvatarLayer only needs to expose a stable transparent
window for OBS to capture. OBS's own
[Virtual Camera Guide](https://obsproject.com/kb/virtual-camera-guide) covers
the `Start Virtual Camera` side of the workflow. To configure a local profile
for OBS Virtual Camera:

```bash
cargo xtask configure-obs-virtual-camera --build
cargo xtask run-metal --release
```

The default OBS preset is recording-performance oriented: MSAA off, texture
mipmaps off, anisotropy 1, diagnostics off, and ScreenCaptureKit probing off.
If the recording path is smooth, `--quality balanced` enables mipmaps while
keeping MSAA off; `--quality high` restores MSAA plus anisotropy 8.

To keep the avatar off the visible desktop while still exposing a normal OBS
Window Capture source, use the offscreen variant:

```bash
cargo xtask configure-obs-virtual-camera --build --offscreen
cargo xtask run-metal --release
```

This still creates a real transparent macOS window named
`MacAvatarLayer OBS Source`, but places it at a far negative screen
coordinate. OBS can select the window; the user should not see it on the
desktop. Use `--desktop` to move the same capture-friendly window back to the
visible desktop.

The same preset is available in the running app from the `MA` menu under
`OBS / Virtual Camera Output` as `Apply OBS Virtual Camera Desktop Preset...`
and `Apply OBS Virtual Camera Offscreen Preset...`. The menu action writes the
active dev/build TOML profile and relaunches the app. The active OBS output
preset uses the same native macOS menu checkmark as the other selectable menu
options; desktop window capture and offscreen window capture are mutually
exclusive.

The offscreen preset is a pragmatic OBS recording path, not a real system camera
source. It still renders a normal WindowServer window for OBS to capture, but
moves that window outside the visible desktop instead of hiding or destroying
it. Recent local stutter testing shows that far-offscreen transparent-window
capture can be less reliable than desktop-visible capture on some OBS/macOS
setups, so treat offscreen as a convenience preset and keep desktop capture as
the stable baseline.

The preset writes the build profile to a transparent `screen_saver` level
window, sets `[app].window_capture_friendly = true`, hides diagnostics, keeps
masks on, applies the selected renderer quality, and disables the
ScreenCaptureKit probe because it is diagnostic-only and not an OBS output path.
The capture-friendly flag gives the avatar window a stable `MacAvatarLayer OBS
Source` title, keeps it as a transparent `NSPanel`, exposes it through normal
app/window enumeration, marks the window as read-only shareable for WindowServer
capture, and moves it outside the visible desktop for the offscreen preset. Use
`--dev` instead of `--build` if you want the same preset in the development
profile.

### ScreenCaptureKit Probe

The first ScreenCaptureKit integration is a runtime sampling probe, not an
output mode. It captures the current avatar window through macOS
ScreenCaptureKit and logs only frame metadata such as frame count, stall, and
recovery events. It does not read pixels, store frames, write images to disk, or
replace the normal transparent window output.

Use it to diagnose whether macOS can keep capturing the avatar window across
Space switches and display sleep/wake:

```toml
[capture.screen_capture_kit]
enabled = true
target_fps = 10
log_interval_seconds = 2.0
stalled_after_seconds = 2.0
```

If the probe reports `permission denied` or `waiting permission`, enable Screen
Recording for `MacAvatarLayer Dev` in macOS System Settings > Privacy &
Security > Screen Recording, then restart the app.

### Why Syphon Was Removed

Syphon support has been removed from this project. Although Syphon is still
useful in some VJ and creative-coding workflows, it is not a good long-term
dependency for this app. The project is focused on a modern macOS rendering
pipeline based on native Metal and standard window capture workflows.

For OBS integration, capture the app window directly with OBS Window Capture or
macOS Screen Capture. Future high-performance capture or frame-sharing work will
prefer ScreenCaptureKit, Metal, or IOSurface instead of reintroducing Syphon.

Pass a different model path as an argument to override local config for that
run:

```bash
cargo xtask run-metal public/CubismSdkForNative/Samples/Resources/Rice/Rice.model3.json
cargo xtask run-metal --release public/CubismSdkForNative/Samples/Resources/Rice/Rice.model3.json
```

List local models before choosing one:

```bash
cargo xtask list-models
cargo xtask list-models public/model
cargo xtask select-model --dev public/model/0.model3.json
cargo xtask select-model --build public/model/0.model3.json
```

`select-model` writes `[model].path` to `mac-avatar-layer.dev.toml` by default;
use `--build` to write `mac-avatar-layer.build.toml`. Later `cargo xtask
run-metal` uses the development config when no model path argument is passed,
while `cargo xtask run-metal --release` uses the build config.
If startup cannot find the selected `.model3.json`, the app prints the active
profile config path and the matching `list-models` / `select-model` command to
repair it before opening the avatar window. When launched with `--config
CUSTOM.toml`, the same error names that custom config and suggests editing its
`[model].path` or passing a command-line model path for that run.
Run `cargo xtask doctor` to check dev/build config files, selected model
manifests, window size settings, renderer, motion, mouse, microphone, camera
input settings, and local Cubism Core SDK paths before launching.

The running app installs a first-pass macOS status bar item named `MA` near the
right side of the menu bar. Its menu shows the active model, expression count,
and renderer quality state, and it can toggle diagnostics, mouse tracking, and
microphone mouth input for the current session. When the model declares
`.exp3.json` expressions, the menu lists them and switches the active expression
without restarting. The menu lists local `.model3.json` files found under
`public/`; selecting one writes `[model].path` to the active dev/build TOML and
relaunches the local `.app` with that model so command-line model overrides do
not keep the old avatar loaded. `Reveal Active Model...` selects the loaded
`.model3.json` in Finder, and `Open Models Folder...` opens the local `public/`
folder, creating it first if needed. The menu also includes Window Size presets
that write `[app].window_width` / `[app].window_height` and relaunch with 100%,
125%, 150%, or 200% sizing.
Renderer Quality presets write `[renderer]` quality fields and relaunch with
Performance, Balanced, or High Quality settings. The menu also includes
Soft/Normal/Expressive mouse, mouth, and camera calibration presets for quick
runtime tuning before committing values to TOML. Use `cargo xtask tune-input`
when you want to persist one of those calibration profiles into the dev/build
TOML before the next launch.
`Open Camera Privacy...` and `Open Microphone Privacy...` jump to the matching
macOS privacy panes for permission repair. `Open Active Config...` opens the
dev/build TOML file that will be used on the next launch. In-process hot model
switching and hot renderer reconfiguration are still planned.

To keep old instances alive during development:

```bash
RUN_METAL_KILL_OLD=0 cargo xtask run-metal
```

Manual equivalent:

```bash
CUBISM_CORE_LIB_DIR="$PWD/public/CubismSdkForNative/Core/lib/macos/arm64" \
CUBISM_CORE_INCLUDE_DIR="$PWD/public/CubismSdkForNative/Core/include" \
  cargo run --features metal-renderer -- public/model/0.model3.json

CUBISM_CORE_LIB_DIR="$PWD/public/CubismSdkForNative/Core/lib/macos/arm64" \
CUBISM_CORE_INCLUDE_DIR="$PWD/public/CubismSdkForNative/Core/include" \
  cargo run --release --features metal-renderer -- public/model/0.model3.json
```

Probe all local models without opening a window:

```bash
cargo xtask probe-risk-models
cargo xtask sample-compatibility-sweep

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
masked offscreens, or inverted masks. `cargo xtask probe-risk-models` writes
`target/render-regression/probe.txt`, and the render regression report embeds
that probe output.
`cargo xtask sample-compatibility-sweep` scans the official SDK sample resources
and writes `target/render-regression/compatibility-sweep.md`, which ranks
models by risk shape and recommends whether the screenshot matrix needs another
stress model beyond `Mao` and `Ren`.

You can also set `LIVE2D_CUBISM_SDK_NATIVE_DIR` to point at a different SDK root.

Capture a cropped Metal renderer screenshot for visual regression checks:

```bash
cargo xtask capture-metal public/CubismSdkForNative/Samples/Resources/Mao/Mao.model3.json
cargo xtask capture-metal public/CubismSdkForNative/Samples/Resources/Ren/Ren.model3.json
cargo xtask capture-risk-models
cargo xtask capture-mask-matrix
cargo xtask mao-mask-audit
cargo xtask capture-offscreen-matrix
cargo xtask ren-offscreen-audit
cargo xtask ren-visual-diff
cargo xtask capture-rice-stress
cargo xtask rice-stress-audit
cargo xtask capture-quality-matrix
cargo xtask quality-visual-diff
cargo xtask capture-full-matrix
```

Screenshots are written to `target/render-regression/`. The capture command
reuses the same SDK auto-detection as `cargo xtask run-metal`, waits for the
app window, captures only that window, and closes the launched process.
`cargo xtask capture-risk-models`
captures the local model plus the SDK `Mao` and `Ren` stress models, preserving
timestamped screenshots and refreshing `latest-*.png` copies for quick visual
comparison after renderer changes. Set `WAIT_SECONDS` if a machine needs longer
for the first build/startup, or `POST_WINDOW_WAIT_SECONDS` if the screenshot
should wait longer for diagnostics and motion to settle after the window appears.
`cargo xtask capture-mask-matrix` temporarily switches the local dev renderer config and
captures the `Mao` stress model in shared-mask, high-precision-mask, and
no-mask modes under `target/render-regression/mask-matrix/`, then restores the
previous `mac-avatar-layer.dev.toml`.
`cargo xtask mao-mask-audit` writes `target/render-regression/mao-mask-audit.md`, a
focused report for Mao's dense clipping, inverted masks, eye masks, capture
references, and manual pass/investigate decision before changing clipping
layout or mask matrix behavior.
`cargo xtask capture-offscreen-matrix` does the same for the `Ren` offscreen/extended
blend stress model under `target/render-regression/offscreen-matrix/`.
`cargo xtask ren-offscreen-audit` writes
`target/render-regression/ren-offscreen-audit.md`, a focused report for Ren's
nested offscreen, masked offscreen, extended offscreen, and extended drawable
distribution, offscreen begin/snapshot/flush timeline, capture references, and
automatic plan checks plus a manual pass/investigate decision before changing
the offscreen compositor.
`cargo xtask ren-visual-diff` writes `target/render-regression/ren-visual-diff.md` and
diff heatmaps under `target/render-regression/ren-visual-diff/`, comparing
shared, high-precision fallback, and no-mask captures across the whole image
plus face/eyes, hair shadow, transparent torso, and pupil offscreen regions.
`cargo xtask capture-rice-stress` captures `Rice` in shared, high-precision, and
no-mask modes under `target/render-regression/rice-stress/` when the SDK sample
is available. Rice is an optional stress model in the full matrix: it covers
additive, inverted-mask, and translucent-drawable risks, and is skipped
automatically when the local SDK sample is missing.
`cargo xtask rice-stress-audit` writes `target/render-regression/rice-stress-audit.md`,
a focused report for Rice's additive drawables, inverted masks, translucent
layering, capture references, and manual pass/investigate decision before
changing blend or mask behavior.
Offscreen models currently fall back from high-precision masks to shared masks;
the overlay marks this as `mask shared(offscreen)`.
The corresponding `high_precision_mask_fallback` renderer event includes
offscreen, masked offscreen, extended offscreen, masked extended drawable,
nested offscreen, and maximum offscreen depth counts.
`cargo xtask capture-quality-matrix` captures the default model plus `Mao` and `Ren`
with texture atlas mipmaps off/on and mipmaps-on-anisotropy-8 under
`target/render-regression/quality-matrix/` so texture shimmer, oblique texture
sampling, and atlas island bleed can be compared.
`cargo xtask quality-visual-diff` writes
`target/render-regression/quality-visual-diff.md` and diff heatmaps under
`target/render-regression/quality-visual-diff/`, comparing mipmaps off/on and
anisotropy 1/8 for the default model, `Mao`, and `Ren` across whole-image and
focused avatar regions. The main render regression report also links the
`mipmaps-on-aniso8` screenshots in its review focus and contact sheet.
Each capture command refreshes `target/render-regression/report.md` unless
`MAC_AVATAR_LAYER_SKIP_REPORT=1` is set for chained captures.
`cargo xtask capture-full-matrix` is the preferred complete visual sweep: it cleans
generated render artifacts once, runs the risk, mask, offscreen, optional Rice
stress, and quality matrices with report generation skipped inside each step,
performs process cleanup between steps, writes Mao/Ren/Rice focused audit
reports plus the Ren and quality visual diff reports, and then writes one final
Markdown report.
The report is a Markdown index with latest screenshot paths, manual review
checklist, embedded thumbnail previews/contact sheet, a structured manual
review record, model risk probe output, automatic review focus, renderer
fallback events, MSAA/edge-quality summaries, focused audit summaries, and
Retina/resize stability summaries from capture logs.
Generated test artifacts under `target/render-regression/` and
`target/space-test/` are cleaned automatically before these commands run. Set
`MAC_AVATAR_LAYER_SKIP_TARGET_CLEAN=1` to keep previous local artifacts for comparison.
Run `cargo xtask clean --all` when you also want to remove Cargo build
outputs.

Metal renderer lifecycle logs use `renderer_event=...` records so Space-switch
and sleep/wake testing can be checked from the terminal. Useful events include
`instance_guard_acquired`, `app_nap_guard_started`, `window_configured`,
`app_active_changed`, `window_visible_changed`, `window_occlusion_changed`,
`metal_initialized`, `contents_scale_changed`, `drawable_size_changed`,
`mask_tile_size_changed`,
`mask_atlas_resized`, `offscreen_texture_size_changed`,
`memory_budget`, `next_drawable_unavailable`, `next_drawable_recovered`,
`long_frame_gap`, and `display_wake_inferred`. `memory_budget` estimates the
renderer-owned atlas, mask, offscreen, MSAA, and blend snapshot texture memory;
Activity Monitor RSS can be higher because it also includes debug runtime,
driver/cache, and system allocation overhead.
The app prevents duplicate local avatar instances with
`target/mac-avatar-layer.pid`; set `MAC_AVATAR_LAYER_ALLOW_DUPLICATE_INSTANCE=1` only
when intentionally debugging multiple windows.
`cargo xtask run-space-test` writes machine logs to `target/space-test/*.log`
and a Markdown checklist/report to `target/space-test/*.md`.

## App Configuration

Runtime options are read once at startup from a profile-specific local config in
the project root. Debug/development runs use `mac-avatar-layer.dev.toml`; release
builds use `mac-avatar-layer.build.toml`. Both files are local-only and ignored
by Git. If the active local config is missing, startup creates it from the
matching committed example before loading. You can also copy or edit
`mac-avatar-layer.dev.example.toml` or `mac-avatar-layer.build.example.toml` when
you want to customize a run.

```toml
[app]
runtime_profile = "development"
window_level = "screen_saver"
window_width = 540.0
window_height = 720.0

[model]
path = "public/model/0.model3.json"

[diagnostics]
show = false

[renderer]
disable_masks = false
high_precision_masks = false
# Defaults depend on [app].runtime_profile:
# development => enable_msaa/log_events true, release => false.
# enable_msaa = true
# log_events = true
atlas_mipmaps = false
atlas_anisotropy = 1
debug_texture_mode = "none"
hidden_drawables = []
hidden_parts = []
only_drawables = []
only_parts = []
highlight_drawables = []
highlight_parts = []

[motion]
# Select by Idle motion index, file name, file stem, or full path.
# idle = "0"
# expression = "smile"
blink_interval = 3.8
blink_duration = 0.18

[input.mouse]
enabled = false
coordinate_space = "screen"
smoothing = 10.0
dead_zone = 0.02
invert_x = false
invert_y = false
eye_x_range = 1.0
eye_y_range = 1.0
angle_x_degrees = 30.0
angle_y_degrees = 22.0
angle_z_degrees = -12.0

[input.microphone]
enabled = false
parameter = "ParamMouthOpenY"
gain = 10.0
noise_gate = 0.008
response_curve = 0.6
smoothing = 18.0
attack = 32.0
release = 10.0
min_open = 0.0
max_open = 1.0

[input.camera]
enabled = false
device = ""
target_fps = 30
pose_mode = "camera_when_available"
smoothing = 12.0
dead_zone = 0.03
invert_x = true
invert_y = false
face_x_offset = 0.0
face_y_offset = 0.0
gaze_x_offset = 0.0
gaze_y_offset = 0.0
roll_offset = 0.0
angle_x_degrees = 30.0
angle_y_degrees = 22.0
angle_z_degrees = 12.0
eye_x_range = 1.0
eye_y_range = 1.0
mouth_enabled = true
mouth_gain = 1.4
mouth_open_offset = 0.0
mouth_min_open = 0.0
mouth_max_open = 1.0
mouth_combine = "max"
blink_from_camera = false
blink_close_threshold = 0.20
blink_open_threshold = 0.38

[overrides]
# mouth_open = 1.0
# mouth_form = 0.0
```

SDK path variables such as `LIVE2D_CUBISM_SDK_NATIVE_DIR`,
`CUBISM_CORE_LIB_DIR`, and `CUBISM_CORE_INCLUDE_DIR` remain build/run command
inputs because they are needed before the app starts.

When a model has multiple motions in the `Idle` group, the first entry remains
the default. Set `[motion].idle` to a zero-based index, file name, file stem, or
full path to choose a different idle motion without editing `model3.json`.
`cargo xtask doctor` validates this value against the selected model and prints
the available `Idle` motion files when it does not match.
Unsupported or malformed `.motion3.json` segment data is reported with the
motion curve target/id and segment offset before that curve is skipped.

Mouse tracking maps the pointer into `ParamEyeBallX/Y` and `ParamAngleX/Y/Z`.
By default `coordinate_space = "screen"`, so the avatar looks across the whole
main display and moving the avatar window does not recenter mouse tracking. Use
`coordinate_space = "window"` when you want tracking to be relative to the
avatar window position. Use `dead_zone`, `invert_x/y`, `eye_*_range`, and
`angle_*_degrees` to tune model-specific feel. Microphone mouth input maps RMS
volume into
`parameter` with separate `attack` and `release` speeds, plus `noise_gate`,
`gain`, `response_curve`, `min_open`, and `max_open` for calibration. Lower
`response_curve` values such as `0.45` make quiet speech open the mouth more;
higher values such as `1.0` behave closer to linear RMS.

Persist a starter tuning profile after trying the session presets in the `MA`
menu:

```bash
cargo xtask tune-input camera expressive
cargo xtask tune-input --build mouth soft
cargo xtask tune-input --build mouse normal
```

Mouse tracking and camera tracking both target head angle and eye-ball
parameters, so `[input.camera].pose_mode` decides which source owns those
parameters:

- `camera_when_available` uses the camera when a face sample exists, then falls
  back to mouse tracking when no face is available.
- `camera` always lets the camera own head/eye pose, even when no face is
  detected.
- `mouse` keeps mouse tracking in charge of head/eye pose while still allowing
  camera mouth tracking to combine with microphone mouth input.

Camera calibration offsets are normalized values applied before camera smoothing
and parameter scaling. Use `face_x_offset` / `face_y_offset` when a centered
face makes the avatar look away from neutral, `gaze_x_offset` /
`gaze_y_offset` when pupil landmarks drift, `roll_offset` when the head tilt
zero point is biased, and `mouth_open_offset` when the camera mouth signal is
too closed or too open at rest. Head tracking prefers Vision `yaw` / `pitch`
when available and falls back to face-center movement otherwise.
`blink_from_camera` enables camera eye-open tracking; `blink_close_threshold`
and `blink_open_threshold` add hysteresis so blinks close decisively without
fluttering while the eyes are half open. When camera blink is active, eye-open
parameters are neutralized only for the lightweight physics sampling step and
then restored before the Cubism update, which avoids blink-driven secondary
body/part pulses on models that wire `ParamEyeLOpen/ROpen` into physics.

Camera tracking is implemented behind the optional `camera-tracking` feature.
`cargo xtask run-metal` always compiles it and launches through
`target/dev-app/MacAvatarLayer Dev.app`, which declares
`NSCameraUsageDescription` / `NSMicrophoneUsageDescription`, signs with
`com.apple.security.device.camera` / `com.apple.security.device.audio-input`,
and uses the stable bundle identifier
`io.github.symphonyiceattack.mac-avatar-layer`. `cargo xtask doctor` checks the
built app wrapper entitlements when the wrapper exists. The active TOML controls
whether the camera opens through `[input.camera].enabled`. If macOS asks for
permission, allow `MacAvatarLayer Dev`, then restart `cargo xtask run-metal`.
If a previous denial is cached, reset just the dev bundle identity and run
again:

```bash
tccutil reset Camera io.github.symphonyiceattack.mac-avatar-layer
cargo xtask run-metal
```

`cargo xtask run-metal` and `cargo xtask build-app` use ad-hoc signing when no
Apple codesigning identity exists. To force a specific identity when multiple
certificates exist:

```bash
security find-identity -v -p codesigning
MAC_AVATAR_LAYER_CODESIGN_IDENTITY="Apple Development: Your Name (TEAMID)" \
  cargo xtask run-metal
```

With `--features camera-tracking`, the app links AVFoundation/Vision through
the `objc2` bindings and probes camera permission plus matching device
availability. If camera permission is still undecided, the native backend asks
macOS for access and logs `renderer_event=camera_permission_response`. The
backend also verifies that an `AVCaptureSession` can be configured with a
camera input, video data output, serial sample callback queue, and
`AVCaptureVideoDataOutputSampleBufferDelegate`. When permission/device setup
succeeds, the native runtime starts the capture session, throttles frames by
`[input.camera].target_fps`, and runs a first-pass
`VNDetectFaceLandmarksRequest` on sample buffers. The delegate maps Vision face
observations into `CameraMotionSample` using bounding box center, roll,
pupil-vs-eye geometry, eye openness, and lip openness. It does not retain,
write, or log frame image data. The mapping is still first-pass and needs
real-camera calibration. When diagnostics are visible, the overlay shows the
current camera status plus face offset, roll, gaze, mouth, and eye sample
values; the `MA` menu also updates the camera status line while the app is
running. Use the same `MA` menu to toggle Camera Tracking on/off at runtime.
Permission denied, no camera, no face, missing backend, and setup failure states
show short menu hints and actionable diagnostics overlay text.

The motion layer already has the safe camera sample contract wired in:
`VNDetectFaceRectanglesRequest` revision 3 supplies native yaw/pitch/roll for
`ParamAngleX/Y/Z`, landmark nose/eye geometry is the yaw fallback, and
`face_offset` remains the last fallback. `gaze` drives `ParamEyeBallX/Y`, and
camera mouth samples can combine with the microphone mouth driver using
`[input.camera].mouth_combine`. Camera yaw/pitch are normalized so roughly 30
degrees of real head turn reaches the configured Live2D head range; roll uses a
roughly 45 degree range. Camera `invert_x` defaults to `true` so webcam tracking
behaves like a mirrored avatar view; set it to `false` if your camera pipeline is
already mirrored. The `MA` menu's Camera Calibration presets scale camera
head/eye ranges and mouth gain for the current session. If Camera Tracking is
off, choosing a preset is remembered and applied the next time the menu toggle
starts camera tracking.

To test the current native camera probe:

```bash
# First set [input.camera].enabled = true in mac-avatar-layer.dev.toml.
cargo xtask run-metal public/model/0.model3.json
```

If microphone input is enabled and macOS denies access, the app prints a
startup diagnostic with the active dev/build config names and the permission
path to repair it. The running app also changes the `MA` menu item to
`Microphone Mouth Input: failed | open Microphone privacy` and shows the first
failure line in the diagnostics overlay when diagnostics are visible.

For lower-overhead build/release-style runs, use `mac-avatar-layer.build.toml`:

```toml
[app]
runtime_profile = "release"
window_level = "screen_saver"
window_width = 540.0
window_height = 720.0

[diagnostics]
show = false
```

In release profile, renderer event logs and MSAA default to off unless
`[renderer].log_events` or `[renderer].enable_msaa` explicitly override them.

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

1. Run `cargo xtask run-space-test`.
2. Move the avatar window where it remains visible.
3. Switch between macOS Desktops/Spaces several times.
4. Watch the diagnostics overlay:
   - `Frames` should keep increasing.
   - `FPS` should recover to roughly 60.
   - `Frame delta max` and `Slow frames` show transition stalls.
5. Confirm startup logs include `renderer_event=app_nap_guard_started` and
   `renderer_event=window_configured`.
   The current overlay policy logs `level_name=screen_saver`, the resolved
   CoreGraphics level, and collection behavior with all-space,
   all-applications, stationary, ignores-cycle, and full-screen auxiliary bits
   enabled. It should also log `kind=nonactivating_panel` and `style_mask=128`.
   `transient` is intentionally not enabled because it caused hidden/double-image
   frames during Space swipe testing. If Space switching hides the avatar
   temporarily, look for `renderer_event=window_reasserted`. If the overlay
   disappears and the frame counter also stops, collect a new report so we can
   distinguish window composition loss from render-loop stalls.
6. Check the terminal for `renderer_event=long_frame_gap` lines and any
   `renderer_event=next_drawable_unavailable` / `next_drawable_recovered`
   pairs.
7. After display sleep/wake, check whether `renderer_event=display_wake_inferred`
   appears and whether FPS/Frames recover afterward.
8. Press `Ctrl-C` in the terminal. The command prints an event summary and
   saves the full log plus a Markdown report under `target/space-test/`.
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
