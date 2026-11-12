# ADR 0070: stereo views of authored depth planes

Status: proposed (local contribution; not offered upstream)

## Context

ADR 0027 already assigns depth to root layers and uses `FOCAL / (FOCAL + depth)` for camera-pan parallax.
The artwork stays two-dimensional and keeps its size.
A stereo view must retain that model, the timeline frame, nested symbols, clips and filters.

## Decision

Evaluate both eyes from the same immutable document, scene and local frame.
Keep ordinary camera transforms, tint, layer order, clips and groups.
After that camera transform, translate each root layer horizontally by half its signed disparity in each direction.
Nested artwork inherits the root placement once.

The host supplies current compositor limits in native stage pixels, after accounting for output width and the displayed view width.
Missing limits do not authorize a fabricated baseline.
With `p = FOCAL / (FOCAL + clamp_depth(depth))`, the disparity (left minus right) is `min(sunk * (p - 1), popped)`: near planes pop, depth zero stays on screen, far planes sink.
This preserves the existing depth ordering and limits.
No toe-in, rotation, scaling or vertical disparity is introduced.
This is a depth-plane stage, not a real three-dimensional camera model.

Zero disparity leaves the ordinary transform untouched.
A frame's image and evaluated scene cache keys both include its eye and limits.
Both results must belong to the requested frame before the host presents one left-first full side-by-side picture, each eye at the stage's own resolution.
Editing onion skin and in-place symbol aids are excluded, as in movie output.
No document edit, output selector or scene eye swap is part of this view state.

## Verification and limits

Pure evaluator tests cover the depth prediction, zero-depth identity, nested artwork, rotated camera and immutable timeline frames.
CPU-render tests cover byte-identical flat output and separate eye/limit cache identities.
Native host wiring and compositor captures are a separate edition change; this API neither chooses a device nor declares a window.
Real three-dimensional transforms, if introduced later, require coherent parallel cameras of one world and a separate projection design.
