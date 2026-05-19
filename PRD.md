# vtube-studio-rs PRD

## Product Goal

Build a Rust-first macOS avatar host inspired by VTube Studio, with the first product advantage being reliable avatar rendering across macOS Desktop/Space switches. The app should load local Live2D Cubism model assets from `public/`, render them through Cubism Core and Metal, and progressively add motion, expression, physics, and tracking input.

## Current State

- The app opens a native borderless AppKit window that can join all Spaces.
- The frame loop runs at roughly 60 FPS with diagnostics for frame timing.
- `public/model/0.model3.json` is loaded locally, including `.moc3`, textures, physics, and display info.
- Cubism Core is integrated through `live2d-cubism-core-sys`.
- Metal rendering supports texture upload, drawable meshes, render order, blend modes, clipping masks, RGBA mask channel packing, 1/2/4/9 mask layout, and explicit affine mask/draw matrices.
- A first `MotionController` drives idle breathing and automatic eye blink, with environment overrides for mouth parameters.

## Non-Goals For The Next Phase

- Do not attempt full VTube Studio feature parity yet.
- Do not commit official Live2D SDK files or model assets to GitHub.
- Do not build a plugin marketplace or scene editor before rendering and motion are stable.
- Do not replace Cubism Core with a pure Rust `.moc3` runtime in this phase.

## Requirements

### 1. Motion, Expression, And Physics

Read and apply `.motion3.json`, `.exp3.json`, and `.physics3.json` so the model is not stuck at static default parameters.

Required outcomes:

- Parse motion file references from `model3.json` when present.
- Parse expression file references from `model3.json` when present.
- Parse `physics3.json` and create a runtime representation of inputs, outputs, particles, gravity, wind, weights, scales, and reflection flags.
- Keep the existing idle controller for breathing and blinking.
- Add a clear update order for parameter writes:
  1. base/default parameters
  2. idle breathing and blink
  3. motion playback
  4. expression overrides/additives
  5. physics outputs
  6. external input overrides
  7. `csmUpdateModel`
- Support a first idle motion loop if the model provides one.
- Keep environment overrides such as `VTUBE_RS_MOUTH_OPEN` useful for debugging.

Acceptance criteria:

- The model blinks automatically without closing permanently.
- `ParamBreath` changes continuously.
- At least one parsed physics output parameter changes over time when its input changes.
- Invalid or unsupported motion/physics files fail gracefully with a readable diagnostic.

### 2. CubismClippingManager Parity

Continue closing the gap with the official Cubism Framework clipping implementation.

Required outcomes:

- Implement the official `ppu / physicalMaskWidth` and `ppu / physicalMaskHeight` precision branches.
- Preserve the current RGBA channel packing and 1/2/4/9 layout behavior.
- Keep explicit `matrix_for_mask` and `matrix_for_draw` structures.
- Add high precision mask mode as an optional path.
- Add support for multiple mask render textures when mask count exceeds one texture's practical capacity.
- Investigate and support offscreen drawables if the Cubism Core model exposes them.

Acceptance criteria:

- Existing sample model still renders without atlas text artifacts or white-eye regressions.
- Masked eye and mouth drawables remain visually stable after window resizing.
- The renderer logs when it falls back because mask count or offscreen features exceed current support.

### 3. Parameter Drivers And Input

Add user/input-driven parameters beyond debug environment variables.

Required outcomes:

- Mouse position drives eye ball and head angle parameters:
  - `ParamEyeBallX`
  - `ParamEyeBallY`
  - `ParamAngleX`
  - `ParamAngleY`
  - `ParamAngleZ`
- Microphone level can drive `ParamMouthOpenY`.
- Automatic blink and breathing remain enabled by default.
- Input drivers can be toggled for debugging.

Acceptance criteria:

- Moving the pointer changes eye/head parameters smoothly.
- A microphone input mode can open/close the mouth based on volume.
- Idle blink/breath does not fight manual or external overrides.

### 4. Rendering Quality

Improve visual fidelity and renderer robustness.

Required outcomes:

- Support per-drawable multiply and screen colors.
- Respect drawable culling and double-sided flags.
- Add mipmap generation for texture atlases.
- Add anisotropic filtering where supported.
- Make mask texture size stable under Retina scale and window resizing.
- Improve transparent window edge antialiasing.

Acceptance criteria:

- Additive, multiplicative, normal, masked, and inverted-mask drawables remain correct.
- Texture sampling remains crisp without shimmering at common window sizes.
- Transparent window corners and avatar edges do not show obvious artifacts.

### 5. macOS Space Stability

Keep the original product focus: avatar rendering should survive desktop switching.

Required outcomes:

- Verify Space switching with the avatar visible on all Spaces.
- Verify behavior beside full-screen apps.
- Confirm App Nap prevention is sufficient during active rendering.
- Keep or refine `NSWindowCollectionBehaviorCanJoinAllSpaces`, `stationary`, and `fullScreenAuxiliary`.
- Detect and recover from Metal layer/device issues after display sleep/wake.
- Add structured trace output for long frame gaps and Space/display transitions.

Acceptance criteria:

- Frame count continues increasing during repeated Space switches.
- FPS recovers after transitions.
- No duplicate app windows remain after reruns.
- Display sleep/wake does not permanently blank the avatar.

### 6. Engineering Experience

Make the project easier to run and debug locally.

Required outcomes:

- Document exact `public/` sample directory layout.
- Document that `public/` is intentionally ignored and not uploaded to GitHub.
- Auto-detect `public/CubismSdkForNative` SDK paths when explicit env vars are absent.
- Add a one-command local run path for Metal renderer.
- Replace some environment-only debug controls with an in-app overlay toggle.
- Keep README and PRD aligned as capabilities change.

Acceptance criteria:

- A developer with local model files and SDK can run the Metal renderer from README instructions.
- Missing SDK/model files produce actionable messages.
- Debug overlay can be hidden/shown without editing code.

## Milestone Plan

### Milestone A: Runtime Parameter Layer

- Keep `motion.rs` as the owner for per-frame parameter updates.
- Add reusable parameter getters/setters by ID.
- Add parameter update ordering.
- Add mouse-driven eye/head controls.
- Add first microphone mouth driver.

### Milestone B: Motion And Expression Files

- Extend `Live2dModel` to parse motions and expressions from `model3.json`.
- Implement minimal `.motion3.json` curve evaluation.
- Implement `.exp3.json` parameter add/multiply/overwrite support.
- Add idle motion selection and looping.

### Milestone C: Physics

- Parse `physics3.json`.
- Implement input normalization, particle simulation, and output application.
- Start with the official algorithm shape from Cubism Framework, adapted to Rust.
- Add diagnostics for physics setting counts and active outputs.

### Milestone D: Clipping Parity

- Add official precision branch logic.
- Add multi texture mask allocation.
- Add high precision path.
- Investigate offscreen drawable APIs and sample compatibility.

### Milestone E: macOS Reliability Pass

- Build a repeatable Space-switch test checklist.
- Add display sleep/wake recovery testing.
- Add structured renderer lifecycle logging.
- Reduce duplicate process/window issues during development.

## Debug Controls

Existing and planned controls:

- `VTUBE_RS_HIDE_DIAGNOSTICS=1`: hide diagnostics overlay.
- `VTUBE_RS_MOUTH_OPEN=<0..1>`: override `ParamMouthOpenY`.
- `VTUBE_RS_MOUTH_FORM=<-1..1>`: override `ParamMouthForm`.
- `VTUBE_RS_BLINK_INTERVAL=<seconds>`: tune automatic blink interval.
- `VTUBE_RS_BLINK_DURATION=<seconds>`: tune blink duration.
- `VTUBE_RS_DISABLE_MASKS=1`: disable masks for renderer debugging.

## Open Questions

- Which motion should be treated as the default idle motion when multiple groups exist?
- Should microphone input use native macOS APIs directly or an audio crate?
- Should webcam/face tracking be built in, bridged from ARKit, or exposed through a plugin API first?
- How much of official Cubism Framework physics should be ported directly versus simplified?
- Should the renderer support software fallback long term, or keep it as a diagnostic path only?
