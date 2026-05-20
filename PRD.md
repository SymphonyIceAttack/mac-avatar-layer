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
  per-drawable multiply/screen colors, double-sided culling flags, optional
  texture-atlas mipmaps/anisotropic sampling, configurable drawable/part hiding,
  and bucketed Retina mask texture sizing for resize stability.
- The Metal layer frame, `contentsScale`, drawable size, mask textures,
  offscreen textures, and MSAA texture are synchronized from the current AppKit
  window bounds/backing scale before each render.
- Cubism-style `ppu / physicalMaskWidth` and `ppu / physicalMaskHeight`
  clipping precision branches are applied when canvas ppu is available.
- Optional high precision masks give each clipping context a full-size mask
  texture and redraw it immediately before each masked drawable instead of
  sharing an RGBA atlas tile.
- Models with Cubism offscreen drawables currently fall back from high precision
  masks to the shared mask path so offscreen render/composite remains enabled;
  diagnostics show this as `mask shared(offscreen)`.
- High precision mask fallback logs include offscreen, masked offscreen,
  extended offscreen, masked extended drawable, nested offscreen, and maximum
  offscreen depth counts so the fallback cause is reviewable from capture logs.
- Shared atlas clipping supports multiple mask render textures when context
  count exceeds one texture's practical capacity.
- Cubism Core offscreen drawable counts are detected, logged, and routed through
  a first-pass Metal offscreen render/composite path.
- The offscreen compositor uses local offscreen item indices instead of assuming
  Cubism Core offscreen indices are contiguous, and nested flush order is
  covered by unit tests.
- Extended blend snapshot timing is covered by unit tests for nested offscreen
  draw targets, parent offscreen composites, and main-target composites.
- Masked offscreen composites use fullscreen quad vertices whose
  `model_position` values are recovered through the inverse `FitTransform`, so
  offscreen mask sampling receives model-space coordinates instead of screen
  NDC coordinates.
- Metal renderer lifecycle events are logged with `renderer_event=...` records
  for startup, drawable size changes, mask/offscreen/MSAA texture changes,
  drawable availability, AppKit active/visible/occlusion state changes, long
  frame gaps, and inferred display wake events.
- Space/display reliability runs write machine logs to `target/space-test/*.log`
  and Markdown checklist reports to `target/space-test/*.md`.
- First Space reliability baseline passed on 2026-05-20 using
  `target/space-test/space-test-20260520-191559.md`: startup guards,
  drawable recovery, and display wake checks passed; two long-frame gaps were
  recorded as transition signals.
- Texture sampling quality can be compared with
  `scripts/capture-quality-matrix.sh`, which captures mipmaps off/on for the
  default model plus `Mao` and `Ren`.
- Render regression captures refresh `target/render-regression/report.md`, a
  Markdown index for latest screenshots, manual visual checks, model risk
  probe output, automatic review focus, renderer fallback events, and recent
  renderer events.
- The render regression report embeds thumbnail previews and grouped contact
  sheets so visual triage can start from the Markdown report before opening
  individual PNG files.
- Render regression report generation now runs through a bounded safe wrapper,
  and `scripts/capture-full-matrix.sh` chains the standard visual matrices with
  cleanup between steps and a single report generation at the end.
- `scripts/capture-rice-stress.sh` captures the official `Rice` sample in
  shared, high-precision, and no-mask modes when the SDK sample is available.
  Rice is treated as an optional stress model for additive, inverted-mask, and
  translucent-drawable coverage, and `scripts/capture-rice-candidate.sh`
  remains as a compatibility alias for the earlier candidate workflow.
- `scripts/ren-offscreen-audit.sh` writes
  `target/render-regression/ren-offscreen-audit.md`, a focused Ren audit for
  nested offscreens, masked offscreens, extended offscreens, and extended
  drawables before changing the offscreen compositor.
- Project-generated `target/` artifacts are cleaned automatically by the local
  run/capture scripts; `scripts/clean-target.sh --all` also removes Cargo build
  outputs when a full cleanup is needed.
- App startup now uses a local PID guard under `target/vtube-studio-rs.pid` to
  prevent duplicate avatar windows during development; set
  `VTUBE_RS_ALLOW_DUPLICATE_INSTANCE=1` only for deliberate debugging.
- Layer edge antialiasing and 4x MSAA are enabled when supported to reduce
  transparent window and avatar mesh edge artifacts.
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
- `--probe-models` scans local `.model3.json` files without opening a window and
  reports parameter, part, drawable, masked drawable, maximum mask, blend-mode,
  inverted-mask, and offscreen counts. It labels models as `risk:low`,
  `risk:medium`, or `risk:high` for renderer compatibility triage, and prints
  specific risk reasons such as dense clipping, many masked drawables, offscreen
  objects, extended blends, masked extended drawables, extended offscreens,
  masked offscreens, or inverted masks.
- `scripts/probe-risk-models.sh` wraps the probe with SDK path auto-detection
  and writes `target/render-regression/probe.txt`; the render regression report
  embeds this output so each screenshot sweep carries the model risk context.
- `scripts/sample-compatibility-sweep.sh` scans the official SDK sample
  resources and writes `target/render-regression/compatibility-sweep.md`, a
  ranked compatibility report for deciding whether more stress models should be
  added to screenshot matrices.
- Local SDK sample probe currently loads 9 models successfully. Notable stress
  cases: `Mao` has 37 masked drawables, which exercises multi shared mask
  textures; `Ren` has 24 offscreen drawables, which exercises the first-pass
  offscreen render/composite path.
- Cubism v5 extended blend modes are decoded in diagnostics as `color + alpha`
  pairs and routed through a first-pass Metal extended blend shader that samples
  a render-target snapshot before compositing.
- Extended alpha compositing now converts snapshot source/destination colors
  back to straight color before applying Over/Atop/Out/Conjoint/Disjoint
  parameters, then writes premultiplied output. This keeps Ren-style extended
  shadow and draw-order composites closer to Cubism Framework behavior.
- Extended color blend ids are mapped from Cubism Core raw color types to the
  Framework shader enum before entering Metal, including AddGlow, Darken,
  Multiply, ColorBurn, LinearBurn, Lighten, Screen, ColorDodge, Overlay,
  SoftLight, HardLight, LinearLight, Hue, and Color.
- Metal startup diagnostics report the number of extended blend objects using
  the extended blend shader.
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
- Keep the official SDK sample compatibility sweep available as the first pass
  before adding new visual regression fixtures.
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

- Preserve the current RGBA channel packing and 1/2/4/9 layout behavior.
- Keep explicit `matrix_for_mask` and `matrix_for_draw` structures.
- Keep masked offscreen composites using inverse-fit model positions for mask
  matrix sampling.
- Keep Cubism Core offscreen drawable detection in runtime diagnostics.
- Continue validating the first-pass Metal offscreen render/composite path
  against `Ren` and future offscreen-heavy sample models.
- Keep nested offscreen flush order covered by tests: child offscreens flush
  into their parent targets before parent targets flush upward.
- Keep extended blend snapshot timing covered by tests before changing snapshot
  texture copies or compositor ordering.
- Refine nested offscreen, offscreen mask, and extended blend parity where
  official sample comparison reveals differences.

Acceptance criteria:

- Existing sample model still renders without atlas text artifacts or white-eye
  regressions.
- Masked eye and mouth drawables remain visually stable after window resizing.
- The renderer logs when it falls back because mask count or offscreen features
  exceed current support.

### 5. Rendering Quality

Status: next visual quality task.

Required outcomes:

- Keep normal, additive, and multiplicative blending visually consistent with
  Cubism Framework behavior.
- Keep per-drawable multiply and screen colors enabled for both masked and
  unmasked drawables.
- Keep culling, double-sided drawables, inverted masks, and clipped drawables
  stable across shared atlas, multi-texture atlas, and high precision mask
  paths.
- Keep atlas mipmaps optional: disabled by default to avoid atlas island bleed,
  but available for well-padded model textures.
- Keep transparent window edges and avatar mesh edges smooth with layer edge
  antialiasing and MSAA where supported.

Acceptance criteria:

- Additive, multiplicative, normal, masked, and inverted-mask drawables remain
  correct.
- Texture sampling remains crisp without shimmering at common window sizes.
- Transparent window corners and avatar edges avoid obvious stair-step artifacts.

### 6. macOS Space Stability

Status: first reliability pass done.

Required outcomes:

- Verify Space switching with the avatar visible on all Spaces.
- Verify behavior beside full-screen apps.
- Confirm App Nap prevention is sufficient during active rendering.
- Keep or refine `NSWindowCollectionBehaviorCanJoinAllSpaces`, `stationary`,
  and `fullScreenAuxiliary`.
- Detect and recover from Metal layer/device issues after display sleep/wake.
- Add structured trace output for long frame gaps and Space/display transitions.
- Log AppKit active, window visibility, and window occlusion state changes while
  running reliability tests.

Acceptance criteria:

- Frame count continues increasing during repeated Space switches.
- FPS recovers after transitions.
- No duplicate app windows remain after reruns.
- Display sleep/wake does not permanently blank the avatar.

Baseline:

- `target/space-test/space-test-20260520-191559.md`
- Automatic assessment: startup guards PASS, drawable recovery PASS, display
  wake PASS, long frame gaps INFO.
- Event counts: `window_occlusion_changed=20`, `app_active_changed=11`,
  `long_frame_gap=2`, `next_drawable_unavailable=0`,
  `display_wake_inferred=0`.

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
- Keep `--probe-models` available for model compatibility checks.
- Keep `scripts/capture-metal.sh` available for cropped renderer screenshots of
  high-risk models.
- Keep `scripts/capture-risk-models.sh` available as the standard local visual
  regression sweep for the default model, `Mao`, and `Ren`.
- Keep `scripts/capture-mask-matrix.sh` available for the `Mao` shared-mask,
  high-precision-mask, and no-mask visual comparison.
- Keep `scripts/capture-offscreen-matrix.sh` available for the `Ren` offscreen
  and extended blend visual comparison.
- Keep `scripts/ren-offscreen-audit.sh` available as the first review step
  before changing nested offscreen, offscreen mask, or extended blend
  compositing behavior.
- Keep `scripts/capture-rice-stress.sh` available as an optional stress sweep
  for additive, inverted-mask, and translucent-drawable risks.
- Keep `scripts/capture-rice-candidate.sh` as a compatibility alias for the
  older candidate workflow.
- Keep `scripts/capture-quality-matrix.sh` available for mipmap/anisotropic
  sampling comparisons on the default model, `Mao`, and `Ren`.
- Keep `scripts/capture-full-matrix.sh` available as the preferred full visual
  regression entry: clean generated artifacts once, run the standard matrices,
  clean stale renderer processes between steps, and generate one final report.
- Keep `scripts/probe-risk-models.sh` available for regenerating the model risk
  probe included in the render regression report.
- Keep `scripts/sample-compatibility-sweep.sh` available for official sample
  compatibility triage.
- Keep `scripts/render-regression-report.sh` available as the common Markdown
  index for render regression screenshots, model risk probe output, and
  automatic review focus.
- Keep `scripts/render-regression-report-safe.sh` available for bounded report
  generation from capture scripts; use `VTUBE_RS_SKIP_REPORT=1` when chaining
  captures manually.
- Keep `scripts/clean-target.sh` available for generated artifact cleanup and
  optional full Cargo target cleanup.
- Keep `scripts/run-space-test.sh` available for repeatable macOS Space and
  display sleep/wake reliability checks with an end-of-run renderer event
  summary and Markdown report.
- Improve missing SDK/model diagnostics.
- Keep diagnostics overlay visibility controlled before startup rather than by
  runtime hotkeys.

### Milestone E: Rendering And Clipping Quality

Status: pending.

- Continue validating per-drawable colors, culling, optional mipmaps,
  anisotropic sampling, bucketed mask texture sizing, and MSAA edge behavior
  against more models.
- Validate multi-texture and high precision mask behavior against high-risk
  models found by `--probe-models`, especially `Mao` and `Ren`.
- Use `target/render-regression/compatibility-sweep.md` to decide whether a
  new official sample model should join the screenshot matrix.
- Treat `Rice` as an optional stress model in the full matrix when the SDK
  sample is present. It specifically covers additive, inverted-mask, and
  translucent-drawable risk not fully covered by `Mao` or `Ren`, and should be
  skipped automatically when the sample is missing.
- Use the `Mao` mask matrix screenshots as the first visual gate for clipping
  changes.
- Use the `Ren` offscreen matrix screenshots as the first visual gate for
  offscreen and extended blend changes.
- Keep Ren risk probe details visible in `target/render-regression/report.md`,
  especially masked extended drawables, extended offscreens, and masked
  offscreens.
- Keep `target/render-regression/ren-offscreen-audit.md` as the focused Ren
  parity report for offscreen render order, nested depth, masks, and extended
  blend distribution.
- Keep high precision mask fallback events visible in
  `target/render-regression/report.md`, especially offscreen mask and nested
  offscreen counts.
- Use the quality matrix screenshots as the first visual gate for optional
  mipmaps/anisotropic sampling changes.
- Use `target/render-regression/report.md` as the standard visual review index.
- Keep the report's Review Focus section as the first place to inspect after a
  matrix run.
- Keep the report's Visual Contact Sheet as the fastest first pass for spotting
  clipping, offscreen, optional Rice stress, and mipmap regressions.
- Keep Retina/window resize behavior covered by Metal layer geometry sync,
  backing-scale sync, and capture logs.

### Milestone F: macOS Reliability Pass

Status: first pass done.

- Keep the repeatable Space-switch checklist and Markdown report workflow.
- Use `scripts/run-space-test.sh` for future display sleep/wake recovery testing
  and event summaries/reports.
- Extend structured renderer lifecycle logging as new macOS failure cases are
  found.
- Continue reducing duplicate process/window issues during development.

## Debug Controls

Runtime debug controls are read once at startup from local
`vtube-studio-rs.toml`. The file is ignored by Git; committed defaults live in
`vtube-studio-rs.example.toml`.

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
