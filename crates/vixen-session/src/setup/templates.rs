//! Element templates, the "Add:" list: `Vixen.Application/Setup/ElementTemplates`. Pure over the working tree, so Preview setup can
//! use them too. Every template creates its head at the root, whatever is selected.

use super::naming::uniquify_in;
use super::tree::{Nid, SetupTree, TreeEffects};
use uuid::Uuid;
use vixen_core::element::{property_ids, Orientation, PropertyData, PropertyInstance};

/// F99, in the order the list shows them.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum TemplateKind {
    Arch,
    CandyCane,
    Icicles,
    IntelligentFixture,
    LipSync,
    Megatree,
    NumberedGroup,
    PixelGrid,
    SingleItem,
    Star,
    Starburst,
}

/// The list, with `TemplateName`s.
pub const ALL_TEMPLATES: [(TemplateKind, &str); 11] = [
    (TemplateKind::Arch, "Arch"),
    (TemplateKind::CandyCane, "Candy Cane"),
    (TemplateKind::Icicles, "Icicles"),
    (TemplateKind::IntelligentFixture, "Intelligent Fixture"),
    (TemplateKind::LipSync, "LipSync"),
    (TemplateKind::Megatree, "Megatree"),
    (TemplateKind::NumberedGroup, "Generic Numbered Group"),
    (TemplateKind::PixelGrid, "Pixel Grid / Matrix"),
    (TemplateKind::SingleItem, "Single Item"),
    (TemplateKind::Star, "Star"),
    (TemplateKind::Starburst, "Starburst"),
];

/// `StartLocation`. Megatree's dialog only offers the two bottom corners, but the generator handles all four.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum StartLocation {
    #[default]
    BottomLeft,
    BottomRight,
    TopLeft,
    TopRight,
}

/// What a template's dialog asks for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TemplateParams {
    Single { name: String },
    Numbered { group: String, prefix: String, count: u32 },
    Starburst { name: String, spokes: u32, spoke_prefix: String, pixel: bool, pixels: u32, pixel_prefix: String },
    Icicles { name: String, strings: u32, pattern: String, string_prefix: String, pixel_prefix: String },
    LipSync { name: String },
    Megatree { name: String, strings: u32, string_prefix: String, pixel: bool, pixels: u32, pixel_prefix: String, start: StartLocation, zig_zag: bool },
    PixelGrid { name: String, horizontal: bool, rows: u32, columns: u32, first_prefix: String, second_prefix: String, start: StartLocation, zig_zag: bool },
}

/// The field defaults each dialog opens with. `None` for Intelligent Fixture, which has a wizard rather than a form.
pub fn template_defaults(kind: TemplateKind) -> Option<TemplateParams> {
    let numbered = |group: &str, prefix: &str, count: u32| TemplateParams::Numbered { group: group.into(), prefix: prefix.into(), count };
    Some(match kind {
        TemplateKind::SingleItem => TemplateParams::Single { name: "New Item".into() },
        TemplateKind::NumberedGroup => numbered("Group", "Item", 10),
        TemplateKind::Arch => numbered("Arch", "Arch Px", 25),
        TemplateKind::CandyCane => numbered("CandyCane", "CandyCane Px", 25),
        TemplateKind::Star => numbered("Star", "Star Px", 40),
        TemplateKind::Starburst => TemplateParams::Starburst {
            name: "Starburst".into(),
            spokes: 8,
            spoke_prefix: "S".into(),
            pixel: false,
            pixels: 50,
            pixel_prefix: "Px".into(),
        },
        TemplateKind::Icicles => TemplateParams::Icicles {
            name: "Icicles".into(),
            strings: 50,
            pattern: "7,9,5".into(),
            string_prefix: "S".into(),
            pixel_prefix: "Px".into(),
        },
        TemplateKind::LipSync => TemplateParams::LipSync { name: "LipSync".into() },
        TemplateKind::Megatree => TemplateParams::Megatree {
            name: "Megatree".into(),
            strings: 16,
            string_prefix: "S".into(),
            pixel: false,
            pixels: 50,
            pixel_prefix: "Px".into(),
            start: StartLocation::BottomLeft,
            zig_zag: false,
        },
        TemplateKind::PixelGrid => TemplateParams::PixelGrid {
            name: "Grid 1".into(),
            horizontal: true,
            rows: 20,
            columns: 32,
            first_prefix: "R".into(),
            second_prefix: "C".into(),
            start: StartLocation::BottomLeft,
            zig_zag: false,
        },
        TemplateKind::IntelligentFixture => return None,
    })
}

/// The read-only number beside "Zig Zag Every". On a Megatree it doesn't look at Pixel Tree, whose disabled fields keep their values (F107).
pub fn zig_zag_every(params: &TemplateParams) -> u32 {
    match params {
        TemplateParams::Megatree { pixels, zig_zag: true, .. } => *pixels,
        TemplateParams::PixelGrid { horizontal, rows, columns, zig_zag: true, .. } => {
            if *horizontal {
                *columns
            } else {
                *rows
            }
        }
        _ => 0,
    }
}

/// `(dimming prompt, colour helper)`: F100 steps 5 and 6.
pub fn template_needs_followups(kind: TemplateKind) -> (bool, bool) {
    let wanted = kind != TemplateKind::IntelligentFixture;
    (wanted, wanted)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TemplateError {
    /// Nothing can be made, which the flow reports as "Could not create elements."
    EmptyName,
    /// F105: the warning stays on the dialog.
    BadPattern,
}

pub const BAD_PATTERN_TITLE: &str = "Invalid String Pattern";
pub const BAD_PATTERN_MESSAGE: &str = "Please enter at least one pixel count in the String Pattern field, e.g. 7,9,5.";

/// The numbers in an Icicles pattern, with empty entries dropped. Upstream's text box stops anything but digits and commas being typed, but a
/// paste gets through and then throws, and values over 32767 overflow `Convert.ToInt16`. Those get the same message as an empty pattern.
pub fn parse_icicle_pattern(pattern: &str) -> Result<Vec<u32>, TemplateError> {
    let mut out = Vec::new();
    for part in pattern.split(',').filter(|p| !p.is_empty()) {
        let value: u32 = part.parse().map_err(|_| TemplateError::BadPattern)?;
        if value > 32767 {
            return Err(TemplateError::BadPattern);
        }
        out.push(value);
    }
    if out.is_empty() {
        return Err(TemplateError::BadPattern);
    }
    Ok(out)
}

/// What the dialog's OK checks before generating.
pub fn validate_template(params: &TemplateParams) -> Result<(), TemplateError> {
    match params {
        TemplateParams::Icicles { pattern, .. } => parse_icicle_pattern(pattern).map(|_| ()),
        _ => Ok(()),
    }
}

#[derive(Clone, Debug, Default)]
pub struct TemplateResult {
    pub head: Option<Nid>,
    /// Every node made, head first: what the follow-up helpers run over.
    pub created: Vec<Nid>,
    pub effects: TreeEffects,
}

/// `GenerateElements`, in one batch. Elements are only made for final leaves, where upstream makes one for every future group and removes it when the
/// first child arrives.
pub fn generate_template(tree: &mut SetupTree, params: &TemplateParams) -> Result<TemplateResult, TemplateError> {
    let mut g = Generator { tree, result: TemplateResult::default() };
    match params {
        TemplateParams::Single { name } => {
            let name = if name.is_empty() { "New Item" } else { name };
            g.create(None, name, true, true);
        }
        TemplateParams::Numbered { group, prefix, count } => {
            if group.is_empty() || prefix.is_empty() {
                return Err(TemplateError::EmptyName);
            }
            let names = g.tree.name_set();
            let head = g.create(None, &uniquify_in(&names, group), *count == 0, false);
            for i in 0..*count {
                g.create(Some(head), &uniquify_in(&names, &format!("{prefix}-{}", i + 1)), true, false);
            }
        }
        TemplateParams::Starburst { name, spokes, spoke_prefix, pixel, pixels, pixel_prefix } => {
            if name.is_empty() {
                return Err(TemplateError::EmptyName);
            }
            g.two_levels(name, *spokes, spoke_prefix, if *pixel { *pixels } else { 0 }, pixel_prefix);
        }
        TemplateParams::Icicles { name, strings, pattern, string_prefix, pixel_prefix } => {
            if name.is_empty() {
                return Err(TemplateError::EmptyName);
            }
            let pattern = parse_icicle_pattern(pattern)?;
            let names = g.tree.name_set();
            let head_name = uniquify_in(&names, name);
            let head = g.create(None, &head_name, *strings == 0, false);
            for i in 0..*strings {
                let pixels = pattern[i as usize % pattern.len()];
                let string_name = uniquify_in(&names, &format!("{head_name} {string_prefix}{}", i + 1));
                let string = g.create(Some(head), &string_name, pixels == 0, false);
                for j in 0..pixels {
                    g.create(Some(string), &uniquify_in(&names, &format!("{string_name} {pixel_prefix}{}", j + 1)), true, false);
                }
            }
        }
        TemplateParams::LipSync { name } => {
            if name.is_empty() {
                return Err(TemplateError::EmptyName);
            }
            // Children are built from the typed name, not the head's final one (F101).
            let head = g.create(None, name, false, true);
            for suffix in ["Outline", "Eyes Open", "Eyes Closed", "Mouth Top", "Mouth Middle", "Mouth Bottom", "Mouth Narrow", "Mouth O"] {
                g.create(Some(head), &format!("{name} {suffix}"), true, true);
            }
        }
        TemplateParams::Megatree { name, strings, string_prefix, pixel, pixels, pixel_prefix, start, zig_zag } => {
            if name.is_empty() {
                return Err(TemplateError::EmptyName);
            }
            let head = g.two_levels(name, *strings, string_prefix, if *pixel { *pixels } else { 0 }, pixel_prefix);
            let every = zig_zag_every(params);
            let order: Option<Vec<Nid>> = match start {
                StartLocation::BottomLeft => zig_zag.then(|| g.leaves(head)),
                StartLocation::BottomRight => Some(g.strings_each_reversed(head)),
                StartLocation::TopLeft => Some(g.strings_reversed(head)),
                StartLocation::TopRight => Some(g.all_leaves_reversed(head)),
            };
            g.apply_order(order, zig_zag.then_some(every));
        }
        TemplateParams::PixelGrid { name, horizontal, rows, columns, first_prefix, second_prefix, start, zig_zag } => {
            if name.is_empty() {
                return Err(TemplateError::EmptyName);
            }
            let (first, second) = if *horizontal { (*rows, *columns) } else { (*columns, *rows) };
            let head = g.two_levels(name, first, first_prefix, second, second_prefix);
            if *horizontal {
                g.tree.node_mut(head).properties.push(PropertyInstance {
                    type_id: property_ids::ORIENTATION,
                    instance_id: Uuid::new_v4(),
                    data: PropertyData::Orientation(Orientation::Horizontal),
                });
            }
            let every = zig_zag_every(params);
            // Upstream numbers horizontal Bottom Right the same as Top Right (PixelGrid.cs:151); here it starts at R1's right end.
            let order: Option<Vec<Nid>> = match (start, *horizontal) {
                (StartLocation::BottomLeft, _) => zig_zag.then(|| g.leaves(head)),
                (StartLocation::TopRight, _) => Some(g.all_leaves_reversed(head)),
                (StartLocation::BottomRight, false) | (StartLocation::TopLeft, true) => Some(g.strings_reversed(head)),
                (StartLocation::BottomRight, true) | (StartLocation::TopLeft, false) => Some(g.strings_each_reversed(head)),
            };
            g.apply_order(order, zig_zag.then_some(every));
        }
    }
    let mut result = g.result;
    result.head = result.created.first().copied();
    Ok(result)
}

struct Generator<'a> {
    tree: &'a mut SetupTree,
    result: TemplateResult,
}

impl Generator<'_> {
    fn create(&mut self, parent: Option<Nid>, name: &str, with_element: bool, uniquify: bool) -> Nid {
        let (id, effects) = self.tree.create_node(parent, Some(name), with_element, uniquify);
        self.result.effects.merge(effects);
        self.result.created.push(id);
        id
    }

    /// Head, `count` children named `"{head} {prefix}{i}"`, and `per` grandchildren each, named from the child.
    fn two_levels(&mut self, name: &str, count: u32, prefix: &str, per: u32, second_prefix: &str) -> Nid {
        let names = self.tree.name_set();
        let head_name = uniquify_in(&names, name);
        let head = self.create(None, &head_name, count == 0, false);
        for i in 0..count {
            let child_name = uniquify_in(&names, &format!("{head_name} {prefix}{}", i + 1));
            let child = self.create(Some(head), &child_name, per == 0, false);
            for j in 0..per {
                self.create(Some(child), &uniquify_in(&names, &format!("{child_name} {second_prefix}{}", j + 1)), true, false);
            }
        }
        head
    }

    fn leaves(&self, node: Nid) -> Vec<Nid> {
        let mut out = Vec::new();
        self.tree.collect_leaves(node, &mut out);
        out
    }

    /// `Children.SelectMany(x => x.GetLeafEnumerator().Reverse())`.
    fn strings_each_reversed(&self, head: Nid) -> Vec<Nid> {
        let mut out = Vec::new();
        for s in &self.tree.node(head).children {
            let mut leaves = self.leaves(*s);
            leaves.reverse();
            out.extend(leaves);
        }
        out
    }

    /// `Children.Reverse().SelectMany(x => x.GetLeafEnumerator())`.
    fn strings_reversed(&self, head: Nid) -> Vec<Nid> {
        let mut out = Vec::new();
        for s in self.tree.node(head).children.iter().rev() {
            self.tree.collect_leaves(*s, &mut out);
        }
        out
    }

    fn all_leaves_reversed(&self, head: Nid) -> Vec<Nid> {
        let mut out = self.leaves(head);
        out.reverse();
        out
    }

    fn apply_order(&mut self, order: Option<Vec<Nid>>, every: Option<u32>) {
        if let Some(nodes) = order {
            match every {
                Some(n) => add_patching_order_zigzag(self.tree, &nodes, n),
                None => add_patching_order(self.tree, &nodes),
            }
        }
    }
}

fn set_order(tree: &mut SetupTree, node: Nid, order: i32) {
    // `Properties.Add` returns null for a property the node already has, so an existing Order isn't overwritten.
    let n = tree.node_mut(node);
    if !n.properties.iter().any(|p| p.type_id == property_ids::ORDER) {
        n.properties.push(PropertyInstance {
            type_id: property_ids::ORDER,
            instance_id: Uuid::new_v4(),
            data: PropertyData::Order { order },
        });
    }
}

/// `OrderModule.AddPatchingOrder(nodes)`: 1, 2, 3, ... in list order.
pub fn add_patching_order(tree: &mut SetupTree, nodes: &[Nid]) {
    for (i, n) in nodes.iter().enumerate() {
        set_order(tree, *n, i as i32 + 1);
    }
}

/// `OrderModule.AddPatchingOrder(nodes, zigZagEvery)`: runs of `every`, odd ones counting up from their first index and even ones down from their last.
pub fn add_patching_order_zigzag(tree: &mut SetupTree, nodes: &[Nid], every: u32) {
    let every = every as i32;
    let (mut total, mut group, mut forward, mut group_start) = (1i32, 0i32, true, 1i32);
    for n in nodes {
        set_order(tree, *n, if forward { total } else { group_start - group });
        group += 1;
        if group >= every {
            forward = !forward;
            group = 0;
            if !forward {
                group_start = total + every;
            }
        }
        total += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(tree: &mut SetupTree, params: &TemplateParams) -> TemplateResult {
        generate_template(tree, params).unwrap()
    }

    fn defaults(kind: TemplateKind) -> TemplateParams {
        template_defaults(kind).unwrap()
    }

    fn name(t: &SetupTree, n: Nid) -> String {
        t.node(n).name.clone()
    }

    fn child_names(t: &SetupTree, n: Nid) -> Vec<String> {
        t.node(n).children.iter().map(|c| name(t, *c)).collect()
    }

    fn order_of(t: &SetupTree, n: Nid) -> Option<i32> {
        t.node(n).properties.iter().find_map(|p| match p.data {
            PropertyData::Order { order } => Some(order),
            _ => None,
        })
    }

    /// Orders in tree order.
    fn orders(t: &SetupTree, head: Nid) -> Vec<Option<i32>> {
        let mut leaves = Vec::new();
        t.collect_leaves(head, &mut leaves);
        leaves.iter().map(|l| order_of(t, *l)).collect()
    }

    fn some(values: &[i32]) -> Vec<Option<i32>> {
        values.iter().map(|v| Some(*v)).collect()
    }

    #[test]
    fn the_list_order_and_default_selection() {
        let names: Vec<&str> = ALL_TEMPLATES.iter().map(|t| t.1).collect();
        assert_eq!(
            names,
            ["Arch", "Candy Cane", "Icicles", "Intelligent Fixture", "LipSync", "Megatree", "Generic Numbered Group", "Pixel Grid / Matrix", "Single Item", "Star", "Starburst"]
        );
    }

    #[test]
    fn followups() {
        assert_eq!(template_needs_followups(TemplateKind::IntelligentFixture), (false, false));
        assert_eq!(template_needs_followups(TemplateKind::Arch), (true, true));
    }

    #[test]
    fn megatree_defaults() {
        let mut t = SetupTree::new();
        let r = run(&mut t, &defaults(TemplateKind::Megatree));
        let head = r.head.unwrap();
        assert_eq!(name(&t, head), "Megatree");
        assert_eq!(t.node(head).element_id, None);
        let children = child_names(&t, head);
        assert_eq!(children.len(), 16);
        assert_eq!(children[0], "Megatree S1");
        assert_eq!(children[15], "Megatree S16");
        assert!(orders(&t, head).iter().all(Option::is_none));
        assert!(t.node(t.node(head).children[0]).element_id.is_some());
        assert_eq!(r.created.len(), 17);
        assert_eq!(r.effects.added_elements.len(), 16);

        let r = run(&mut t, &defaults(TemplateKind::Megatree));
        let head = r.head.unwrap();
        assert_eq!(name(&t, head), "Megatree - 2");
        assert_eq!(child_names(&t, head)[0], "Megatree - 2 S1");
    }

    fn pixel_tree(start: StartLocation, zig_zag: bool) -> TemplateParams {
        TemplateParams::Megatree {
            name: "T".into(),
            strings: 3,
            string_prefix: "S".into(),
            pixel: true,
            pixels: 2,
            pixel_prefix: "P".into(),
            start,
            zig_zag,
        }
    }

    #[test]
    fn megatree_bottom_right_orders_each_string_backwards() {
        let mut t = SetupTree::new();
        let head = run(&mut t, &pixel_tree(StartLocation::BottomRight, false)).head.unwrap();
        // S1P1, S1P2, S2P1, S2P2, S3P1, S3P2 in tree order.
        assert_eq!(orders(&t, head), some(&[2, 1, 4, 3, 6, 5]));
        let mut t = SetupTree::new();
        let head = run(&mut t, &pixel_tree(StartLocation::BottomRight, true)).head.unwrap();
        // Walk order is S1P2, S1P1, S2P2, S2P1, S3P2, S3P1, numbered 1, 2, 4, 3, 5, 6.
        assert_eq!(orders(&t, head), some(&[2, 1, 3, 4, 6, 5]));
    }

    #[test]
    fn megatree_bottom_left() {
        let mut t = SetupTree::new();
        let head = run(&mut t, &pixel_tree(StartLocation::BottomLeft, false)).head.unwrap();
        assert!(orders(&t, head).iter().all(Option::is_none));
        let mut t = SetupTree::new();
        let head = run(&mut t, &pixel_tree(StartLocation::BottomLeft, true)).head.unwrap();
        assert_eq!(orders(&t, head), some(&[1, 2, 4, 3, 5, 6]));
    }

    #[test]
    fn megatree_top_corners_exist_in_the_generator() {
        let mut t = SetupTree::new();
        let head = run(&mut t, &pixel_tree(StartLocation::TopLeft, false)).head.unwrap();
        assert_eq!(orders(&t, head), some(&[5, 6, 3, 4, 1, 2]));
        let mut t = SetupTree::new();
        let head = run(&mut t, &pixel_tree(StartLocation::TopRight, false)).head.unwrap();
        assert_eq!(orders(&t, head), some(&[6, 5, 4, 3, 2, 1]));
    }

    #[test]
    fn megatree_order_ignores_pixel_tree() {
        let plain_tree = |start, zig_zag| TemplateParams::Megatree {
            name: "T".into(),
            strings: 3,
            string_prefix: "S".into(),
            pixel: false,
            pixels: 2,
            pixel_prefix: "P".into(),
            start,
            zig_zag,
        };
        // Each string is a leaf, numbered by what the disabled fields still hold.
        let mut t = SetupTree::new();
        let head = run(&mut t, &plain_tree(StartLocation::BottomLeft, true)).head.unwrap();
        assert_eq!(orders(&t, head), some(&[1, 2, 4]));
        let mut t = SetupTree::new();
        let head = run(&mut t, &plain_tree(StartLocation::BottomRight, false)).head.unwrap();
        assert_eq!(orders(&t, head), some(&[1, 2, 3]));
        let mut t = SetupTree::new();
        let head = run(&mut t, &plain_tree(StartLocation::BottomLeft, false)).head.unwrap();
        assert!(orders(&t, head).iter().all(Option::is_none));
        assert_eq!(zig_zag_every(&plain_tree(StartLocation::BottomLeft, true)), 2);
    }

    #[test]
    fn pixel_grid_defaults() {
        let mut t = SetupTree::new();
        let r = run(&mut t, &defaults(TemplateKind::PixelGrid));
        let head = r.head.unwrap();
        assert_eq!(name(&t, head), "Grid 1");
        assert_eq!(r.created.len(), 661);
        assert_eq!(r.effects.added_elements.len(), 640);
        assert_eq!(child_names(&t, head)[0], "Grid 1 R1");
        assert_eq!(child_names(&t, t.node(head).children[0])[0], "Grid 1 R1 C1");
        let orientation = t.node(head).properties.iter().find(|p| p.type_id == property_ids::ORIENTATION).unwrap();
        assert_eq!(orientation.data, PropertyData::Orientation(Orientation::Horizontal));
    }

    fn grid(horizontal: bool, start: StartLocation, zig_zag: bool) -> TemplateParams {
        let (first, second) = if horizontal { ("R", "C") } else { ("C", "R") };
        TemplateParams::PixelGrid {
            name: "G".into(),
            horizontal,
            rows: 2,
            columns: 2,
            first_prefix: first.into(),
            second_prefix: second.into(),
            start,
            zig_zag,
        }
    }

    #[test]
    fn pixel_grid_vertical_has_columns_first_and_no_orientation() {
        let mut t = SetupTree::new();
        let params = TemplateParams::PixelGrid {
            name: "Grid 1".into(),
            horizontal: false,
            rows: 20,
            columns: 32,
            first_prefix: "C".into(),
            second_prefix: "R".into(),
            start: StartLocation::BottomLeft,
            zig_zag: false,
        };
        let head = run(&mut t, &params).head.unwrap();
        assert_eq!(child_names(&t, head).len(), 32);
        assert_eq!(child_names(&t, head)[0], "Grid 1 C1");
        assert_eq!(child_names(&t, t.node(head).children[0]).len(), 20);
        assert_eq!(child_names(&t, t.node(head).children[0])[0], "Grid 1 C1 R1");
        assert!(t.node(head).properties.is_empty());
    }

    #[test]
    fn pixel_grid_every_corner_against_the_table() {
        use StartLocation::*;
        // Leaves in tree order: S1L1, S1L2, S2L1, S2L2.
        let cases = [
            (true, BottomRight, [2, 1, 4, 3]),
            (true, TopLeft, [3, 4, 1, 2]),
            (true, TopRight, [4, 3, 2, 1]),
            (false, BottomRight, [3, 4, 1, 2]),
            (false, TopLeft, [2, 1, 4, 3]),
            (false, TopRight, [4, 3, 2, 1]),
        ];
        for (horizontal, start, expected) in cases {
            let mut t = SetupTree::new();
            let head = run(&mut t, &grid(horizontal, start, false)).head.unwrap();
            assert_eq!(orders(&t, head), some(&expected), "{horizontal} {start:?}");
        }
        for horizontal in [true, false] {
            let mut t = SetupTree::new();
            let head = run(&mut t, &grid(horizontal, BottomLeft, false)).head.unwrap();
            assert!(orders(&t, head).iter().all(Option::is_none));
            let mut t = SetupTree::new();
            let head = run(&mut t, &grid(horizontal, BottomLeft, true)).head.unwrap();
            assert_eq!(orders(&t, head), some(&[1, 2, 4, 3]));
        }
    }

    #[test]
    fn zig_zag_numbers_nine_nodes_in_threes() {
        let mut t = SetupTree::new();
        let nodes: Vec<Nid> = (0..9).map(|i| t.create_node(None, Some(&format!("N{i}")), true, true).0).collect();
        add_patching_order_zigzag(&mut t, &nodes, 3);
        let values: Vec<i32> = nodes.iter().map(|n| order_of(&t, *n).unwrap()).collect();
        assert_eq!(values, [1, 2, 3, 6, 5, 4, 7, 8, 9]);
    }

    #[test]
    fn an_existing_order_is_not_overwritten() {
        let mut t = SetupTree::new();
        let n = t.create_node(None, Some("N"), true, true).0;
        add_patching_order(&mut t, &[n]);
        add_patching_order_zigzag(&mut t, &[n, n], 1);
        assert_eq!(order_of(&t, n), Some(1));
        assert_eq!(t.node(n).properties.len(), 1);
    }

    #[test]
    fn numbered_groups() {
        let mut t = SetupTree::new();
        let head = run(&mut t, &defaults(TemplateKind::Star)).head.unwrap();
        assert_eq!(name(&t, head), "Star");
        let kids = child_names(&t, head);
        assert_eq!((kids[0].as_str(), kids[39].as_str(), kids.len()), ("Star Px-1", "Star Px-40", 40));
        let head = run(&mut t, &defaults(TemplateKind::Star)).head.unwrap();
        assert_eq!(name(&t, head), "Star - 2");
        assert_eq!(child_names(&t, head)[0], "Star Px-1 - 2");

        let mut t = SetupTree::new();
        let head = run(&mut t, &defaults(TemplateKind::CandyCane)).head.unwrap();
        assert_eq!((name(&t, head).as_str(), child_names(&t, head).len()), ("CandyCane", 25));
        let head = run(&mut t, &defaults(TemplateKind::Arch)).head.unwrap();
        assert_eq!(child_names(&t, head).len(), 25);
        let head = run(&mut t, &defaults(TemplateKind::NumberedGroup)).head.unwrap();
        assert_eq!((name(&t, head).as_str(), child_names(&t, head)[0].as_str()), ("Group", "Item-1"));
        assert_eq!(child_names(&t, head)[9], "Item-10");
    }

    #[test]
    fn an_empty_name_makes_nothing() {
        let mut t = SetupTree::new();
        let params = TemplateParams::Numbered { group: "G".into(), prefix: String::new(), count: 2 };
        assert_eq!(generate_template(&mut t, &params).unwrap_err(), TemplateError::EmptyName);
        assert!(t.is_empty());
    }

    #[test]
    fn icicles() {
        let mut t = SetupTree::new();
        let r = run(&mut t, &defaults(TemplateKind::Icicles));
        let head = r.head.unwrap();
        assert_eq!(t.node(head).children.len(), 50);
        let mut leaves = Vec::new();
        t.collect_leaves(head, &mut leaves);
        assert_eq!(leaves.len(), 352);
        let sizes: Vec<usize> = t.node(head).children.iter().take(4).map(|s| t.node(*s).children.len()).collect();
        assert_eq!(sizes, [7, 9, 5, 7]);
        assert_eq!(child_names(&t, t.node(head).children[0])[0], "Icicles S1 Px1");

        assert_eq!(parse_icicle_pattern("7,,9").unwrap(), [7, 9]);
        assert_eq!(parse_icicle_pattern("").unwrap_err(), TemplateError::BadPattern);
        assert_eq!(parse_icicle_pattern(",,").unwrap_err(), TemplateError::BadPattern);
        assert_eq!(parse_icicle_pattern("abc").unwrap_err(), TemplateError::BadPattern);
        assert_eq!(parse_icicle_pattern("40000").unwrap_err(), TemplateError::BadPattern);

        // A zero makes a string that is a leaf.
        let params = TemplateParams::Icicles { name: "I".into(), strings: 2, pattern: "0,1".into(), string_prefix: "S".into(), pixel_prefix: "P".into() };
        let head = run(&mut t, &params).head.unwrap();
        let first = t.node(head).children[0];
        assert!(t.is_leaf(first));
        assert!(t.node(first).element_id.is_some());
    }

    #[test]
    fn lipsync_uses_the_typed_name_for_its_children() {
        let mut t = SetupTree::new();
        let head = run(&mut t, &defaults(TemplateKind::LipSync)).head.unwrap();
        assert_eq!(
            child_names(&t, head),
            ["LipSync Outline", "LipSync Eyes Open", "LipSync Eyes Closed", "LipSync Mouth Top", "LipSync Mouth Middle", "LipSync Mouth Bottom", "LipSync Mouth Narrow", "LipSync Mouth O"]
        );
        let head = run(&mut t, &defaults(TemplateKind::LipSync)).head.unwrap();
        assert_eq!(name(&t, head), "LipSync - 2");
        assert_eq!(child_names(&t, head)[0], "LipSync Outline - 2");
    }

    #[test]
    fn single_item() {
        let mut t = SetupTree::new();
        let a = run(&mut t, &TemplateParams::Single { name: String::new() }).head.unwrap();
        let b = run(&mut t, &TemplateParams::Single { name: "New Item".into() }).head.unwrap();
        assert_eq!((name(&t, a).as_str(), name(&t, b).as_str()), ("New Item", "New Item - 2"));
    }

    #[test]
    fn starburst() {
        let mut t = SetupTree::new();
        let head = run(&mut t, &defaults(TemplateKind::Starburst)).head.unwrap();
        let kids = child_names(&t, head);
        assert_eq!((kids.len(), kids[0].as_str(), kids[7].as_str()), (8, "Starburst S1", "Starburst S8"));
        let params = TemplateParams::Starburst { name: "B".into(), spokes: 1, spoke_prefix: "S".into(), pixel: true, pixels: 2, pixel_prefix: "Px".into() };
        let head = run(&mut t, &params).head.unwrap();
        assert_eq!(child_names(&t, t.node(head).children[0]), ["B S1 Px1", "B S1 Px2"]);
    }

    #[test]
    fn zig_zag_number() {
        assert_eq!(zig_zag_every(&pixel_tree(StartLocation::BottomLeft, true)), 2);
        assert_eq!(zig_zag_every(&pixel_tree(StartLocation::BottomLeft, false)), 0);
        let wide = TemplateParams::PixelGrid {
            name: "G".into(),
            horizontal: true,
            rows: 3,
            columns: 7,
            first_prefix: "R".into(),
            second_prefix: "C".into(),
            start: StartLocation::BottomLeft,
            zig_zag: true,
        };
        assert_eq!(zig_zag_every(&wide), 7);
    }

    #[test]
    #[ignore = "a time budget, for release builds"]
    fn a_1000_by_100_grid_takes_under_a_second() {
        let mut t = SetupTree::new();
        let params = TemplateParams::PixelGrid {
            name: "G".into(),
            horizontal: true,
            rows: 1000,
            columns: 100,
            first_prefix: "R".into(),
            second_prefix: "C".into(),
            start: StartLocation::BottomLeft,
            zig_zag: false,
        };
        let start = std::time::Instant::now();
        let r = run(&mut t, &params);
        let spent = start.elapsed();
        assert_eq!(r.created.len(), 101_001);
        assert!(spent.as_secs_f64() < 1.0, "{spent:?}");
    }
}
