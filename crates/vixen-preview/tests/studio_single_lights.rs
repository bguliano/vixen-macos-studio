//! Studio: bulbs the Single Light tool places for selected elements (B60) keep their place when the preview opens again, the
//! x = y diagonal included (B64).

use std::sync::Arc;
use vixen_core::ElementTree;
use vixen_preview::data::PreviewData;
use vixen_preview::point::PreviewPoint;
use vixen_preview::session::studio_single_lights::SINGLE_LIGHT_SPACING;
use vixen_preview::session::{Change, DesignSession};

#[test]
fn bulbs_on_the_diagonal_survive_reopening() {
    let mut tree = ElementTree::new();
    let group = tree.add_string("Str", 3);
    let tree = Arc::new(tree);
    let mut s = DesignSession::new(PreviewData::new(), Some(Arc::clone(&tree)));
    let mut ch = Change::default();
    // The first bulb lands on (100, 100), where left, right, top and bottom are one number.
    let ids = s.studio_add_single_lights(PreviewPoint::new(100, 100), &[tree.node(group).id], 800, &mut ch);
    assert_eq!(ids.len(), 3);
    let at = |s: &DesignSession, i: usize| {
        let shape = &s.item(ids[i]).unwrap().shape;
        (shape.left(), shape.top())
    };
    assert_eq!(at(&s, 0), (100, 100), "placed on the diagonal, not nudged off it");

    let mut reopened = DesignSession::new(s.into_data(), Some(Arc::clone(&tree)));
    reopened.set_background_size(800, 600);
    let (_, report) = reopened.setup_open();
    assert!(report.removed.is_empty(), "a Single light on x = y is not a sizeless shape added by mistake");
    for i in 0..3 {
        assert_eq!(at(&reopened, i), (100 + SINGLE_LIGHT_SPACING * i as i32, 100));
    }
}
