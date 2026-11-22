//! Effect modules: butterfly, circles, color_wash, curtain, fire, garlands, life, plasma, snow_storm, spiral, spirograph, tree. Generated tables live in `generated`.

pub mod generated;
pub mod butterfly;
pub mod circles;
pub mod color_wash;
pub mod curtain;
pub mod fire;
pub mod garlands;
pub mod life;
pub mod plasma;
pub mod snow_storm;
pub mod spiral;
pub mod spirograph;
mod studio_spiral_steps; // STUDIO: tests for Spiral's Step on Marks (B51)
pub mod tree;
#[cfg(test)]
mod test_util;

use vixen_effects::framework::EffectDescriptor;

pub fn descriptors() -> Vec<&'static EffectDescriptor> {
    vec![&butterfly::DESCRIPTOR, &circles::DESCRIPTOR, &color_wash::DESCRIPTOR, &curtain::DESCRIPTOR, &fire::DESCRIPTOR, &garlands::DESCRIPTOR, &life::DESCRIPTOR, &plasma::DESCRIPTOR, &snow_storm::DESCRIPTOR, &spiral::DESCRIPTOR, &spirograph::DESCRIPTOR, &tree::DESCRIPTOR, ]
}
