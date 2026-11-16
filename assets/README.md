# Avatar frame artwork

`avatar-frame.png` is the application-ready derivative of the user-supplied
`Marco de avatar ciberpunk pixelado.png`. The original stays unchanged in the
project root; it is not needed at runtime or to build the embedded derivative.

## Preparation

The original is 1254 × 1254 RGB, with an opaque checkerboard rather than alpha.
Source SHA-256:
`11415b61b7de0a6f0fef3017ef05c5ba3ff96de54fb4059adc1012b27e792f98`.

The derivative removes only connected near-neutral light background regions,
seeded from the outside edges and center: minimum RGB channel at least 178,
maximum minus minimum at most 24, four-neighbor flood fill. Removed pixels have
zero RGBA; disconnected highlights and colored artwork retain their original
RGB and full alpha. Cropping to the remaining artwork yields:

- Source crop: x=77, y=81, width=1100, height=1092.
- PNG: 1100 × 1092, 8-bit RGBA; 322119 foreground pixels.
- Clear content rectangle: x=125, y=121, width=851, height=851, within the crop.
  This centered square is inset two source pixels from the first opaque edge.

The clear rectangle is deliberately conservative so the complete avatar PNG,
including its labels and virtual top padding, cannot be covered by the frame.
Do not replace this with whole-image black/white color-keying or remove opaque
black pixels from incoming avatar images.

## Rendering contract

The desktop embeds and decodes this asset once on its first opaque multi-avatar
render, uploads one shared texture, and uses nearest-neighbor sampling. Fit the
whole frame proportionally inside each tile; fit the complete avatar/labels in
the clear rectangle, then paint the frame above it. Transparent mode uses no
frame and preserves its earlier image geometry. A single avatar stays borderless.
No sender PNG, protocol, destination or preference is changed.

Each slider step sizes the window so the frame's clear opening contains the
avatar's whole composition (image plus its one source separation row) at the
chosen multiplier plus three logical points of snapping slack; the whole-pixel
snapped fit then renders the avatar at the chosen step's physical level (below
one physical pixel per source texel the fractional contain fit applies).
The user visually accepted the rendered frames and compact menu in a native
screenshot with two avatars. Layout at all five avatar-size slider steps is
tested; that
screenshot alone does not establish every platform, DPI or focus behavior.
