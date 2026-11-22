//! The effect framework: descriptors, parameters and render context, the pixel effect driver, the pulse
//! renderer many basic effects share, and a System.Drawing-style canvas. The effects themselves live in
//! the `fx-*` crates and are collected by `vixen-effect-registry`. Porting rules: docs/PORTING_GUIDE.md.

pub mod framework;
pub mod gfx;
pub mod pixel;
pub mod pulse_renderer;
pub mod studio_mark_steps; // STUDIO: beat-quantized movement for effects that step on marks (#67)
pub mod testing;

/// Everything an effect module needs.
pub mod prelude {
    pub use crate::framework::*;
    pub use crate::pixel::*;
    pub use vixen_core::color::{clamp, clip_value};
    pub use vixen_core::{
        Color, ColorGradient, ColorPoint, Curve, DiscreteValue, EffectIntents, ElementTree, FrameArray, GradientLevelPair,
        Hsv, Intent, IntentData, IntentNode, LightingValue, Mark, MarkCollection, NodeId, RgbValue, TimeSpan, Uuid,
    };
}
