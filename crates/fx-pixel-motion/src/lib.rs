//! Effect modules: balls, bars, fireworks, meteors, pin_wheel, shockwave. Generated tables live in `generated`.

mod audio;
pub mod generated;
pub mod balls;
pub mod bars;
pub mod fireworks;
pub mod meteors;
pub mod pin_wheel;
pub mod shockwave;
mod studio_bars_steps; // STUDIO: tests for Bars' Step on Marks (#67)

use vixen_effects::framework::EffectDescriptor;

pub fn descriptors() -> Vec<&'static EffectDescriptor> {
    vec![&balls::DESCRIPTOR, &bars::DESCRIPTOR, &fireworks::DESCRIPTOR, &meteors::DESCRIPTOR, &pin_wheel::DESCRIPTOR, &shockwave::DESCRIPTOR, ]
}

/// Helpers for the effect tests: render a grid of strings and read back one frame.
#[cfg(test)]
pub(crate) mod test_util {
    use vixen_effects::framework::{EffectDescriptor, Params};
    use vixen_effects::testing;
    use vixen_core::{ElementTree, IntentData, RgbValue, TimeSpan};

    /// Renders `strings` x `pixels` with default parameters edited by `edit`, and returns the colour of
    /// every pixel at `frame` as `[string][pixel]`. Frames are 50 ms.
    pub fn frame(
        d: &'static EffectDescriptor,
        strings: usize,
        pixels: usize,
        ms: i64,
        frame: i64,
        edit: impl FnOnce(&mut Params),
    ) -> Vec<Vec<RgbValue>> {
        let mut tree = ElementTree::new();
        let g = testing::grid(&mut tree, "G", strings, pixels);
        let out = testing::render_with(d, &tree, &[g], ms, edit);
        let elements = tree.distinct_elements(g);
        let mut res = vec![vec![RgbValue::BLACK; pixels]; strings];
        for (i, e) in elements.iter().enumerate() {
            if let Some(nodes) = out.nodes_for(e) {
                if let IntentData::ArrayRgb(a) = &nodes[0].intent.data {
                    res[i / pixels][i % pixels] = a.at(TimeSpan::from_ms_i(frame * 50));
                }
            }
        }
        res
    }

    /// Like `frame`, but one render and every frame of it: `[frame][string][pixel]`.
    pub fn all_frames(
        d: &'static EffectDescriptor,
        strings: usize,
        pixels: usize,
        ms: i64,
        edit: impl FnOnce(&mut Params),
    ) -> Vec<Vec<Vec<RgbValue>>> {
        let mut tree = ElementTree::new();
        let g = testing::grid(&mut tree, "G", strings, pixels);
        let out = testing::render_with(d, &tree, &[g], ms, edit);
        let elements = tree.distinct_elements(g);
        let n = (ms / 50) as usize;
        let mut res = vec![vec![vec![RgbValue::BLACK; pixels]; strings]; n];
        for (i, e) in elements.iter().enumerate() {
            if let Some(nodes) = out.nodes_for(e) {
                if let IntentData::ArrayRgb(a) = &nodes[0].intent.data {
                    for (f, frame) in res.iter_mut().enumerate() {
                        frame[i / pixels][i % pixels] = a.at(TimeSpan::from_ms_i(f as i64 * 50));
                    }
                }
            }
        }
        res
    }

    pub fn lit(f: &[Vec<RgbValue>]) -> usize {
        f.iter().flatten().filter(|c| c.r > 0 || c.g > 0 || c.b > 0).count()
    }
}
