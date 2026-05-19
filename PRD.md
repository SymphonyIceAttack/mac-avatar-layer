# vtube-studio-rs PRD

## Product Goal

Build a Rust-first macOS avatar host inspired by VTube Studio, with the first
product advantage being reliable avatar rendering across macOS Desktop/Space
switches. The app should load local Live2D Cubism model assets from `public/`,
render them through Cubism Core and Metal, and progressively add motion,
expression, physics, and tracking input.

## Current State

- The app opens a native borderless AppKit window that can join all Spaces.
- The frame loop runs at roughly 60 FPS with diagnostics for frame timing.
- `public/model/0.model3.json` is loaded locally, including `.moc3`, textures,
  physics, display info, motions, and expressions when present.
- `public/` is intentionally local-only and ignored by Git.
- Cubism Core is integrated through `live2d-cubism-core-sys`.
- Metal rendering supports texture upload, drawable meshes, render order,
  normal/additive/multiplicative blend modes, clipping masks, RGBA mask channel
  packing, 1/2/4/9 mask layout, explicit affine mask/draw matrices,
  per-drawable multiply/screen colors, double-sided culling flags, mipmapped
  texture atlases, and anisotropic atlas sampling.
- `MotionController` owns per-frame parameter updates in this order:
  1. idle blink and breath
  2. idle `.motion3.json` playback
  3. optional `.exp3.json` expression
  4. mouse and microphone input
  5. `.physics3.json` output
  6. local TOML config overrides
  7. `csmUpdateModel`
- Motion first pass is implemented: model3 motion references, idle motion
  selection, looping, and linear/bezier/stepped/inverse-stepped segments.
- Expression first pass is implemented: `Add`, `Multiply`, and `Overwrite`
  parameter blends selected by local TOML config.
- Physics first pass is implemented: input normalization, particles, gravity,
  wind, stabilization, fixed-step evaluation, output interpolation, and
  diagnostics.
- Input first pass is implemented: mouse-driven head/eye parameters and
  microphone RMS-driven `ParamMouthOpenY`.
- `scripts/run-metal.sh` provides a one-command local Metal run path with SDK
  path detection for `public/CubismSdkForNative`.
- Runtime diagnostics, renderer debug switches, input drivers, and manual mouth
  overrides are configured before launch from `vtube-studio-rs.toml`.

## Non-Goals For The Next Phase

- Do not attempt full VTube Studio feature parity yet.
- Do not commit official Live2D SDK files or model assets to GitHub.
- Do not build a plugin marketplace or scene editor before rendering and motion
  are stable.
- Do not replace Cubism Core with a pure Rust `.moc3` runtime in this phase.

## Requirements

### 1. Motion, Expression, And Physics

Status: first pass done.

Required outcomes:

- Parse motion file references from `model3.json` when present.
- Parse expression file references from `model3.json` when present.
- Parse `physics3.json` and create a runtime representation of inputs, outputs,
  particles, gravity, wind, weights, scales, and reflection flags.
- Keep idle breathing and blinking enabled by default.
- Keep local config overrides such as `overrides.mouth_open` useful for
  debugging.

Remaining work:

- Add broader compatibility tests against more official SDK sample models.
- Add better diagnostics for unsupported motion segment data.
- Add expression fade-in/fade-out if needed by later UI controls.

### 2. Parameter Drivers And Input

Status: first pass done.

Required outcomes:

- Mouse position drives:
  - `ParamEyeBallX`
  - `ParamEyeBallY`
  - `ParamAngleX`
  - `ParamAngleY`
  - `ParamAngleZ`
- Microphone level drives `ParamMouthOpenY`.
- Automatic blink and breathing remain enabled by default.
- Input drivers can be toggled for debugging.

Remaining work:

- Calibrate mouse tracking ranges per model.
- Calibrate microphone gain/noise gate defaults on more machines.
- Consider native macOS permission messaging if microphone startup fails.

### 3. Engineering Experience

Status: in progress.

Required outcomes:

- Document exact `public/` sample directory layout.
- Document that `public/` is intentionally ignored and not uploaded to GitHub.
- Auto-detect `public/CubismSdkForNative` SDK paths when explicit env vars are
  absent.
- Add a one-command local run path for Metal renderer.
- Keep diagnostics overlay visibility as a launch-time configuration.
- Keep runtime debug controls in `vtube-studio-rs.toml`, not process
  environment variables.
- Keep README and PRD aligned as capabilities change.

Acceptance criteria:

- A developer with local model files and SDK can run the Metal renderer from
  README instructions.
- Missing SDK/model files produce actionable messages.
- Debug overlay can be hidden/shown without editing code.

### 4. CubismClippingManager Parity

Status: next major rendering task.

Required outcomes:

- Implement the official `ppu / physicalMaskWidth` and
  `ppu / physicalMaskHeight` precision branches.
- Preserve the current RGBA channel packing and 1/2/4/9 layout behavior.
- Keep explicit `matrix_for_mask` and `matrix_for_draw` structures.
- Add high precision mask mode as an optional path.
- Add support for multiple mask render textures when mask count exceeds one
  texture's practical capacity.
- Investigate and support offscreen drawables if Cubism Core exposes them.

Acceptance criteria:

- Existing sample model still renders without atlas text artifacts or white-eye
  regressions.
- Masked eye and mouth drawables remain visually stable after window resizing.
- The renderer logs when it falls back because mask count or offscreen features
  exceed current support.

### 5. Rendering Quality

Status: next visual quality task.

Required outcomes:

- Make mask texture size stable under Retina scale and window resizing.
- Improve transparent window edge antialiasing.

Acceptance criteria:

- Additive, multiplicative, normal, masked, and inverted-mask drawables remain
  correct.
- Texture sampling remains crisp without shimmering at common window sizes.
- Transparent window corners and avatar edges do not show obvious artifacts.

### 6. macOS Space Stability

Status: dedicated reliability pass pending.

Required outcomes:

- Verify Space switching with the avatar visible on all Spaces.
- Verify behavior beside full-screen apps.
- Confirm App Nap prevention is sufficient during active rendering.
- Keep or refine `NSWindowCollectionBehaviorCanJoinAllSpaces`, `stationary`,
  and `fullScreenAuxiliary`.
- Detect and recover from Metal layer/device issues after display sleep/wake.
- Add structured trace output for long frame gaps and Space/display transitions.

Acceptance criteria:

- Frame count continues increasing during repeated Space switches.
- FPS recovers after transitions.
- No duplicate app windows remain after reruns.
- Display sleep/wake does not permanently blank the avatar.

## Milestone Plan

### Milestone A: Runtime Parameter Layer

Status: first pass done.

- `motion.rs` owns per-frame parameter updates.
- Parameter getters/setters by ID are available through the Cubism wrapper.
- Mouse-driven eye/head controls are implemented.
- Microphone mouth driver is implemented.

### Milestone B: Motion And Expression Files

Status: first pass done.

- `Live2dModel` parses motions and expressions from `model3.json`.
- Minimal `.motion3.json` curve evaluation is implemented.
- `.exp3.json` parameter add/multiply/overwrite support is implemented.
- Idle motion selection and looping are implemented.

### Milestone C: Physics

Status: first pass done.

- `physics3.json` is parsed.
- Input normalization, particle simulation, stabilization, fixed-step updates,
  and output interpolation are implemented.
- Diagnostics log physics setting/output counts.

### Milestone D: Engineering Experience

Status: active.

- Keep README/PRD updated.
- Keep `scripts/run-metal.sh` as the recommended local run entry.
- Improve missing SDK/model diagnostics.
- Keep diagnostics overlay visibility controlled before startup rather than by
  runtime hotkeys.

### Milestone E: Rendering And Clipping Quality

Status: pending.

- Continue validating per-drawable colors, culling, mipmaps, and anisotropic
  sampling against more models.
- Add official clipping precision branches.
- Add high precision and multi-texture mask paths.
- Improve Retina/window resize behavior.

### Milestone F: macOS Reliability Pass

Status: pending.

- Build a repeatable Space-switch test checklist.
- Add display sleep/wake recovery testing.
- Add structured renderer lifecycle logging.
- Reduce duplicate process/window issues during development.

## Debug Controls

Runtime debug controls are read once at startup from local
`vtube-studio-rs.toml`. The file is ignored by Git; committed defaults live in
`vtube-studio-rs.example.toml`.

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

## Open Questions

- Which motion should be treated as the default idle motion when multiple
  `Idle` motions exist?
- Should microphone permission failures get an in-window prompt or only terminal
  diagnostics for now?
- Should webcam/face tracking be built in, bridged from ARKit, or exposed
  through a plugin API first?
- How far should the project port official Cubism Framework physics beyond the
  current lightweight implementation?
- Should the renderer support software fallback long term, or keep it as a
  diagnostic path only?
