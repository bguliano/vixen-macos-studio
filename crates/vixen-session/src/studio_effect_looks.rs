//! What each effect looks like on common props, in plain words (B81), so an agent picks the right effect the first time
//! instead of rendering a gallery to find out. effect_catalog and effects_library return it as `looks`.
//!
//! Two facts explain most surprises, and the texts below repeat them where they matter:
//! - Basic effects that step through "consecutive elements" (Chase, Spin, and Alternating/Dissolve by default) step
//!   through a group's lights in element order: on a megatree, string 1 pixel by pixel, then string 2. Their
//!   DepthOfEffect 1 steps whole strings instead, which is what goes around a tree.
//! - Pixel effects draw a picture with one column per string (S1 on the left) and one row per light, row 1 being each
//!   string's Px1, at the bottom of the picture. On a megatree the picture is wrapped around the trunk: columns go
//!   around the tree, rows go up it. A tree wired from the top (display_geometry's px1 "top") shows it upside down,
//!   unless TargetPositioning is Locations, which draws by where the lights sit.

/// The look of `effect` (its registered name), or None for effects with nothing to see (commands, data) we haven't described.
pub fn looks(effect: &str) -> Option<&'static str> {
    Some(match effect {
        "Set Level" => "Every target light at one steady colour and level for the whole effect. On a megatree, the whole tree at once. The simplest base layer.",
        "Pulse" => "Every target light together, following LevelCurve over the effect's length (a fade in, out or a swell) in ColorGradient's colours over time. On a megatree the whole tree breathes as one; no movement.",
        "Strobe" => "Every target light flashing on and off together at a steady rate. Whole-prop; use sparingly on hits.",
        "Twinkle" => "Each light flickers on and off at random times, independently, like sparkles. On a megatree, scattered points all over the tree; MinimumLevel/MaximumLevel and coverage set how busy it is. Calm and safe for long passages.",
        "Candle Flicker" => "Each light wavers in brightness at random like a candle flame. Subtle, warm; best on single bulbs, mini-tree lines and window props.",
        "Chase" => "A pulse moves through the targets' lights one after another in element order. On a megatree group that means up string 1 pixel by pixel, then string 2 and so on: at any moment only a short sliver of one string is lit, so it does NOT read as movement around the tree. To chase around the tree, set DepthOfEffect 1 (whole strings step in turn); for movement up the tree use Bars, Wipe or Garlands. Good as-is on a single string, arch or roofline: a run of light along it.",
        "Spin" => "Pulses step through the targets' lights in element order, repeating RevolutionCount times. On a megatree group with the default DepthOfEffect 0 that is pixel by pixel along one string after another, so only a one-string sliver is lit at a time and it looks like a flicker on one side, not a spin. Set DepthOfEffect 1 so whole strings step in turn: that is the classic spin around the trunk. On an arch or line it runs along it.",
        "Alternating" => "Lights alternate between two colours/levels, swapping over time or on marks. By default it alternates pixel by pixel; to alternate whole strings of a tree (stripes around it) set EnableDepth true, DepthOfEffect 1, GroupLevel 1.",
        "Dissolve" => "Lights switch on (or off) one at a time in random order until the whole target is lit (or dark): a sparkly fill. On a megatree, random pixels all over; with EnableDepth and DepthOfEffect 1, whole strings appear at random.",
        "Wipe" => "A band of light sweeps across the targets by their Location (lights without Vixen's Location stay dark; element_location gives them one). Direction Horizontal goes left to right; plain Vertical moves DOWNWARD (top to bottom): set ReverseDirection for bottom to top. Burst, Circle and Diamond spread from the centre outward. On a megatree the sweep follows the preview picture: Vertical rises or falls through the tree, Horizontal crosses it left to right as seen from the front (both sides at once, not around). WipeMovement Movement with a flat MovementCurve parks the band, which is how effect_band draws still rings.",
        "Colorwash" => "The whole target in one colour that changes through ColorGradient over time; optional fades across the picture. On a megatree, the whole tree changes colour together. A smooth background wash.",
        "Bars" => "Stripes of the palette's colours moving across the picture. Direction Up/Down: horizontal rings that climb or fall on a megatree (with Px1 at the bottom). Left/Right: vertical stripes that travel around the tree. Expand/Compress: rings from the middle outward or inward. Good beat-driven movement on trees and matrices.",
        "Garlands" => "Rows of light (drooping garland shapes) stacking up or falling down the picture. On a megatree, horizontal swags that build up the tree; Direction picks up or down.",
        "Spiral" => "Diagonal stripes that twist across the picture. On a megatree they wrap around the trunk like a barber pole or candy cane, turning as its movement settings say. One of the best whole-tree movements.",
        "Curtain" => "A curtain of light opening or closing from the edges or the centre of the picture. On a megatree the picture's edges are S1 and the last string and its centre the middle strings, so it opens around the tree from wherever those strings hang (display_geometry gives S1's angle). On a matrix or window, a real curtain.",
        "Meteors" => "Streaks with fading tails falling (or rising, or exploding outward) through the picture. On a megatree, meteors run down the strings: shooting stars on the tree. Count and length set density.",
        "Snowstorm" => "Dots with short trails drifting down the picture at random. On a megatree, snow falling down the tree; calm.",
        "Snowflakes" => "Snowflake-shaped sprites falling down the picture. Reads on a matrix; on a megatree they become falling blobs.",
        "Fire" => "Flames rising from the bottom of the picture (Location Bottom; Top, Left, Right flip it). On a megatree, the tree burns from its base up; on a matrix, a fire.",
        "Wave" => "A sine wave travelling across the picture. On a megatree it rolls around the tree as a wavy band; on a matrix or roofline a moving wave.",
        "Pinwheel" => "Arms rotating about a centre point of the picture. Shaped for a matrix or star; on a megatree the unrolled picture makes it diagonal sweeps rather than a clean pinwheel.",
        "Shockwave" => "A ring expanding from a centre point of the picture. On a matrix, a ripple; on a megatree, a band spreading from the middle of the tree's height outward.",
        "Circles" => "Circles or bouncing balls moving around the picture. Reads on matrices; on a megatree, blobs drifting over the tree.",
        "Balls" => "Balls moving and bouncing around the picture. Best on a matrix; on a megatree, dots roaming over it.",
        "Butterfly" => "A flowing, symmetrical colour pattern that moves continuously. Fills a matrix or megatree with busy colour; good for energetic sections.",
        "Plasma" => "Smooth moving blobs of colour blending into each other. A soft, continuous fill on a matrix or megatree.",
        "Life" => "Conway's Game of Life: cells being born and dying on the picture's grid. Reads on a matrix; random sparkle on a tree.",
        "Spirograph" => "Spirograph curves drawn and turning on the picture. Matrix-shaped; on a megatree, looping lines over it.",
        "Tree" => "A tree-shaped pattern of branches drawn on the picture, made for megatrees: layered branch rows from the base up.",
        "Whirlpool" => "A swirling vortex pattern. Best on a matrix; on a megatree, twisting colour.",
        "Pattern" => "A repeating pattern (dots, stripes, chevrons) moving across the picture.",
        "Liquid" => "Particles flowing like liquid, poured from emitters and pooling under gravity. Matrix effect.",
        "Morph" => "A line or band moving from one position to another across the picture (a sweeping bar between two points).",
        "Shapes" => "Shapes (hearts, stars, circles and more) appearing, growing or moving on the picture. Matrix effect.",
        "Border" => "A frame around the edge of the picture. On a matrix, its outline; on a megatree, the first and last strings plus the bottom and top rows.",
        "Text" => "Text scrolling or standing on the picture. Needs a matrix tall enough for the font; on a megatree it wraps around the tree.",
        "Count Down" => "A number counting down, drawn as text on the picture. Matrix effect.",
        "Picture" => "An image shown, moved or zoomed on the picture. Matrix effect; a file parameter, so agents can't set it.",
        "Video" => "A video played on the picture. Matrix effect; a file parameter, so agents can't set it.",
        "Glediator" => "Plays a Glediator recording on the picture. Matrix effect; a file parameter, so agents can't set it.",
        "VU Meter" => "Brightness follows the music's volume: every target light together, louder is brighter.",
        "Vertical Meter" => "A bar that rises with the music's volume, from the bottom of the picture. On a megatree the lit height follows the volume.",
        "Waveform" => "The audio waveform drawn across the picture as it plays. Matrix or long-line effect.",
        "LipSync" => "Lights a face prop's mouth shapes from phoneme marks (needs Face mapping on the elements).",
        "State" => "Lights a prop's named states (shapes on a state-based prop) from marks or values.",
        "Custom Value" => "Sends one raw value to the targets' channels (for DMX and special channels). Nothing visible on pixels unless the channel is a colour.",
        "Fixture" | "Fixture Strobe" | "Set Position" | "Set Zoom" | "Gobo" | "Prism" | "Frost" | "Spin Color Wheel" | "Line Dance" => "For intelligent (moving-head) fixtures only; nothing visible on pixel props.",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_registered_effect_with_a_look_is_named_exactly() {
        let names: Vec<&str> = vixen_effect_registry::all().iter().map(|d| d.name).collect();
        for key in ["Wipe", "Spin", "Chase", "Bars", "Spiral", "Pulse", "Twinkle", "Meteors", "Fire", "Colorwash"] {
            assert!(names.contains(&key), "{key} isn't a registered effect name");
            assert!(super::looks(key).is_some(), "{key} has no look");
        }
        let described = names.iter().filter(|n| super::looks(n).is_some()).count();
        assert!(described + 3 >= names.len(), "only {described} of {} effects described", names.len());
        assert!(super::looks("Wipe").unwrap().contains("DOWNWARD"));
        assert!(super::looks("Spin").unwrap().contains("sliver"));
    }
}
