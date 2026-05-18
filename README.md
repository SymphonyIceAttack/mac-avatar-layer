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
- Draws a small animated Core Animation placeholder layer as the first avatar
  stand-in.
- Uses no third-party crates yet, so the base can compile without network
  dependency downloads.

## Run

```bash
cargo run
```

Use `Ctrl-C` in the terminal to stop the prototype.

## Why This Solves The First Problem

VTube-style apps often appear to freeze on macOS when their render/update work
is tied too closely to normal window focus, normal Space membership, or UI
callbacks that stop firing when the app is no longer active in the current
Desktop.

This prototype separates the first critical pieces:

- The avatar window is allowed to join every Space.
- The window stays floating and auxiliary to full-screen Spaces.
- The frame loop is owned by the process, not only by a visible focused window.

## Next Milestones

1. Add a Metal or wgpu renderer behind the avatar layer.
2. Add a small diagnostics overlay showing FPS, active Space transitions, and
   frame latency.
3. Load a simple 2D avatar format before integrating Live2D Cubism.
4. Add camera/face-tracking input as a separate thread with timestamped frames.
5. Implement the VTube Studio plugin WebSocket API compatibility layer.
