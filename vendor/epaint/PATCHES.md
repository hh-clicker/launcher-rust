# HH Clicker font rendering patch

Upstream: epaint 0.31.1 from the locked Cargo registry archive, part of https://github.com/emilk/egui/tree/0.31.1/crates/epaint . MIT OR Apache-2.0; both upstream licenses are included here.

Changes are limited to:

- `src/image.rs`: font atlas coverage gamma defaults to 1.0 instead of 0.55. Keep the geometric antialiasing coverage rather than expanding partially covered edges.
- `src/text/font.rs`: preserve fractional physical font size instead of rounding the em size to an integer. Glyph advances, kerning and outlines use the same size.
- `src/text/fonts.rs`: key the font cache by the actual scale bits, so different fractional sizes cannot reuse the same rounded entry.
- `src/tessellator.rs`: annotate an upstream float literal as f32 for Rust 1.100 compatibility; no behavior change.

The renderer remains egui_glow/OpenGL. Inter fonts and application colors are unchanged. This patch is local to this launcher; Cargo's global/registry sources are not modified.
