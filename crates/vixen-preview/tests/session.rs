//! The design surface, driven the way a host drives it: as a sequence of mouse and keyboard events.

use std::sync::Arc;
use uuid::Uuid;
use vixen_core::element::{property_ids, PropertyData, PropertyInstance};
use vixen_core::{Color, ElementColorType, ElementTree};
use vixen_preview::data::{PreviewData, ShapeId};
use vixen_preview::link::ElementLinker;
use vixen_preview::overlay::theme;
use vixen_preview::point::PreviewPoint;
use vixen_preview::session::{apply_location_updates, Change, Cursor, DesignSession, Key, MenuItem, MouseButton, Tool};
use vixen_preview::shape::properties::{PropertyError, PropertyValue};
use vixen_preview::shape::{Modifiers, Shape, StringType};
use vixen_preview::template::TemplateStore;
use vixen_xml::XElement;

const NONE: Modifiers = Modifiers::NONE;
const CTRL: Modifiers = Modifiers::CTRL;
const SHIFT: Modifiers = Modifiers::SHIFT;

fn p(x: i32, y: i32) -> PreviewPoint {
    PreviewPoint::new(x, y)
}

fn hline(x: i32, y: i32, w: i32) -> Shape {
    Shape::new_line(p(x, y), p(x + w, y), 11, None, 1.0)
}

/// Three lines, 100 long, one above another, 50 apart, all starting at x = 100.
fn three_lines() -> (DesignSession, ShapeId, ShapeId, ShapeId) {
    let mut d = PreviewData::new();
    let a = d.add_shape(hline(100, 100, 100));
    let b = d.add_shape(hline(100, 150, 100));
    let c = d.add_shape(hline(100, 200, 100));
    (DesignSession::new(d, None), a, b, c)
}

fn click(s: &mut DesignSession, x: i32, y: i32, m: Modifiers) {
    s.mouse_down(x, y, m);
    s.mouse_up(x, y, m);
}

fn drag(s: &mut DesignSession, from: (i32, i32), to: (i32, i32), m: Modifiers) -> Change {
    s.mouse_down(from.0, from.1, m);
    let ch = s.mouse_move(to.0, to.1, m);
    s.mouse_up(to.0, to.1, m);
    ch
}

fn lights_of(s: &DesignSession, id: ShapeId) -> Vec<(f32, f32)> {
    let r = s.scene().range_of(id).unwrap();
    s.scene().lights[r].iter().map(|l| (l.x, l.y)).collect()
}

/// Where the renderer's view puts a shape's lights, scroll aside.
fn on_screen(s: &DesignSession, id: ShapeId) -> Vec<(f32, f32)> {
    let zoom = s.scene().zoom() as f32;
    lights_of(s, id).iter().map(|l| (l.0 * zoom, l.1 * zoom)).collect()
}

fn left_top(s: &DesignSession, id: ShapeId) -> (i32, i32) {
    let sh = &s.item(id).unwrap().shape;
    (sh.left(), sh.top())
}

/// The scene must always match what a fresh build of the data would give.
fn assert_scene_fresh(s: &DesignSession) {
    let fresh = vixen_preview::scene::Scene::build_at_zoom(s.data(), s.tree().map(|t| &**t), s.zoom());
    assert_eq!(s.scene().lights, fresh.lights);
    assert_eq!(s.scene().light_count(), fresh.light_count());
    for i in 0..fresh.light_count() {
        assert_eq!(s.scene().node_of_light(i), fresh.node_of_light(i));
    }
}

// ----- hit testing and selection

#[test]
fn clicking_selects_what_is_under_the_mouse_and_elsewhere_deselects() {
    let (mut s, a, b, _) = three_lines();
    assert_eq!(s.selected_item(), None);
    click(&mut s, 150, 150, NONE);
    assert_eq!(s.selected_item(), Some(b));
    click(&mut s, 150, 100, NONE);
    assert_eq!(s.selected_item(), Some(a));
    click(&mut s, 500, 500, NONE);
    assert_eq!(s.selected_item(), None);
    assert!(s.selected_items().is_empty());
}

#[test]
fn hit_rectangles_are_exactly_the_csharp_ones() {
    let (mut s, a, _, _) = three_lines();
    // A light at (100, 100), size 3: from (97, 97) to (106, 106) inclusive.
    click(&mut s, 97, 97, NONE);
    assert_eq!(s.selected_item(), Some(a));
    click(&mut s, 96, 97, NONE);
    assert_eq!(s.selected_item(), None);
    click(&mut s, 97, 96, NONE);
    assert_eq!(s.selected_item(), None);
}

#[test]
fn selecting_puts_up_handles() {
    let (mut s, _, b, _) = three_lines();
    assert!(s.overlay().squares.is_empty());
    let ch = s.mouse_down(150, 150, NONE);
    assert!(ch.selection_changed && ch.overlay_changed);
    assert_eq!(ch.show_properties, Some(b));
    s.mouse_up(150, 150, NONE);
    let o = s.overlay();
    let at: Vec<[f32; 2]> = o.squares.iter().map(|h| h.center).collect();
    assert_eq!(at, [[100.0, 150.0], [200.0, 150.0]]);
    assert!(o.squares.iter().all(|h| h.size == 6.0 && h.color == Color::WHITE));
    assert_eq!(o.circles.len(), 1);
    assert_eq!(o.circles[0].center, [150.0, 140.0], "the rotate handle, 10 above the top");
    assert_eq!(o.circles[0].radius, 3.0);
    let ch = s.mouse_down(500, 500, NONE);
    s.mouse_up(500, 500, NONE);
    assert!(ch.clear_properties);
    assert!(s.overlay().squares.is_empty());
}

#[test]
fn the_cursor_follows_what_is_under_the_mouse() {
    let (mut s, _, _, _) = three_lines();
    assert_eq!(s.mouse_move(150, 150, NONE).cursor, None, "nothing selected, nothing to tell");
    click(&mut s, 150, 150, NONE);
    assert_eq!(s.mouse_move(150, 150, NONE).cursor, Some(Cursor::SizeAll));
    assert_eq!(s.mouse_move(100, 150, NONE).cursor, Some(Cursor::Cross));
    assert_eq!(s.mouse_move(150, 140, NONE).cursor, Some(Cursor::Rotate));
    assert_eq!(s.mouse_move(400, 400, NONE).cursor, Some(Cursor::Default));
    s.set_tool(Tool::Arch);
    assert_eq!(s.mouse_move(400, 400, NONE).cursor, Some(Cursor::InsertMode));
}

#[test]
fn control_click_builds_a_multi_selection() {
    let (mut s, a, b, c) = three_lines();
    click(&mut s, 150, 100, CTRL);
    click(&mut s, 150, 150, CTRL);
    assert_eq!(s.selected_item(), None, "no single item, no handles");
    assert_eq!(s.selected_items(), [a, b]);
    assert!(s.is_selected(a) && s.is_selected(b) && !s.is_selected(c));
    assert!(s.overlay().squares.is_empty());
    // Control-clicking a member takes it back out.
    click(&mut s, 150, 100, CTRL);
    assert_eq!(s.selected_items(), [b]);
}

#[test]
fn control_click_adds_the_single_selected_item_to_the_multi_selection() {
    let (mut s, a, b, _) = three_lines();
    click(&mut s, 150, 100, NONE);
    assert_eq!(s.selected_item(), Some(a));
    click(&mut s, 150, 150, CTRL);
    assert_eq!(s.selected_item(), None);
    assert_eq!(s.selected_items(), [a, b]);
}

#[test]
fn clicking_away_clears_a_multi_selection() {
    let (mut s, _, _, _) = three_lines();
    click(&mut s, 150, 100, CTRL);
    click(&mut s, 150, 150, CTRL);
    click(&mut s, 500, 500, NONE);
    assert!(s.selected_items().is_empty());
}

#[test]
fn select_all_takes_every_unlocked_shape() {
    let (mut s, a, b, c) = three_lines();
    s.edit_shape(b, |sh| sh.set_locked(true));
    let ch = s.select_all();
    assert!(ch.selection_changed);
    assert_eq!(s.selected_items(), [a, c]);
    let ch = s.clear_selection();
    assert!(ch.selection_changed);
    assert!(s.selected_items().is_empty());
}

#[test]
fn select_all_of_one_shape_makes_it_the_selected_item() {
    let mut d = PreviewData::new();
    let a = d.add_shape(hline(0, 0, 10));
    let mut s = DesignSession::new(d, None);
    s.select_all();
    assert_eq!(s.selected_item(), Some(a));
    assert!(s.selected_items().is_empty());
}

// ----- the rubber band

#[test]
fn rubber_band_left_to_right_needs_the_whole_shape_inside() {
    let (mut s, a, b, c) = three_lines();
    s.mouse_down(50, 50, NONE);
    assert!(s.mouse_captured());
    // Round the first two lines completely, and through the third.
    let ch = s.mouse_move(250, 175, NONE);
    assert!(ch.selection_changed);
    let o = s.overlay();
    let band = o.rubber_band.unwrap();
    assert_eq!((band.x, band.y, band.width, band.height), (50.0, 50.0, 200.0, 125.0));
    assert_eq!(band.color, Color::WHITE);
    assert_eq!(s.selected_items(), [a, b]);
    // Shrink it so the second line is only partly inside.
    s.mouse_move(150, 175, NONE);
    assert_eq!(s.selected_items(), [] as [ShapeId; 0], "left to right, part inside is not enough");
    s.mouse_move(250, 225, NONE);
    assert_eq!(s.selected_items(), [a, b, c]);
    s.mouse_up(250, 225, NONE);
    assert_eq!(s.selected_items(), [a, b, c], "it stays selected after the mouse comes up");
    assert_eq!(s.selected_item(), None);
    assert!(s.overlay().rubber_band.is_none());
}

#[test]
fn rubber_band_right_to_left_takes_anything_touched() {
    let (mut s, a, b, _) = three_lines();
    s.mouse_down(150, 50, NONE);
    s.mouse_move(120, 170, NONE);
    assert_eq!(s.selected_items(), [a, b]);
    s.mouse_up(120, 170, NONE);
    assert_eq!(s.selected_items(), [a, b]);
}

#[test]
fn rubber_band_ending_with_one_shape_selects_it_singly() {
    let (mut s, a, _, _) = three_lines();
    drag(&mut s, (50, 50), (250, 120), NONE);
    assert_eq!(s.selected_item(), Some(a));
    assert!(s.selected_items().is_empty());
    assert_eq!(s.overlay().squares.len(), 2);
}

#[test]
fn rubber_band_skips_locked_shapes_unless_shift_is_down() {
    let (mut s, a, b, _) = three_lines();
    s.edit_shape(a, |sh| sh.set_locked(true));
    drag(&mut s, (50, 50), (250, 175), NONE);
    assert_eq!(s.selected_item(), Some(b));
    click(&mut s, 500, 500, NONE);
    drag(&mut s, (50, 50), (250, 175), SHIFT);
    assert_eq!(s.selected_items(), [a, b]);
}

// ----- moving

#[test]
fn dragging_moves_only_that_shape_and_reports_only_its_range() {
    let (mut s, a, b, c) = three_lines();
    let (la, lc) = (lights_of(&s, a), lights_of(&s, c));
    // The first press only selects, as in the C#. It takes a second one on the selected shape to drag.
    let first = drag(&mut s, (150, 150), (160, 160), NONE);
    assert!(first.scene.is_empty());
    assert_eq!(left_top(&s, b), (100, 150));
    s.mouse_down(150, 150, NONE);
    let ch = s.mouse_move(170, 180, NONE);
    assert_eq!(ch.scene.len(), 1);
    assert_eq!(ch.scene[0].shape, Some(b));
    assert_eq!(ch.scene[0].range, 11..22);
    assert!(!ch.scene[0].resized && !ch.scene[0].links_changed);
    s.mouse_up(170, 180, NONE);
    assert_eq!(left_top(&s, b), (120, 180));
    assert_eq!(lights_of(&s, b)[0], (120.0, 180.0));
    assert_eq!(lights_of(&s, a), la);
    assert_eq!(lights_of(&s, c), lc);
    assert_scene_fresh(&s);
}

#[test]
fn move_undo_and_redo() {
    let (mut s, _, b, _) = three_lines();
    let before = lights_of(&s, b);
    click(&mut s, 150, 150, NONE);
    drag(&mut s, (150, 150), (170, 180), NONE);
    click(&mut s, 500, 500, NONE);
    assert_eq!(s.num_undoable(), 1);
    assert_eq!(s.undo_descriptions(), ["Move 1 "]);
    let moved = lights_of(&s, b);

    let ch = s.undo();
    assert!(ch.undo_changed && !ch.scene.is_empty());
    assert_eq!(lights_of(&s, b), before);
    assert_eq!(left_top(&s, b), (100, 150));
    assert_eq!((s.num_undoable(), s.num_redoable()), (0, 1));
    assert_scene_fresh(&s);

    s.redo();
    assert_eq!(lights_of(&s, b), moved);
    assert_eq!(left_top(&s, b), (120, 180));
    assert_eq!((s.num_undoable(), s.num_redoable()), (1, 0));
    assert_scene_fresh(&s);

    // Undo again, on what redo restored, and the same comes back.
    s.undo();
    assert_eq!(lights_of(&s, b), before);
    // And nothing left to undo is harmless.
    s.undo();
    assert_eq!(lights_of(&s, b), before);
}

#[test]
fn a_selected_shape_loses_its_handles_on_undo_as_in_the_csharp() {
    let (mut s, _, b, _) = three_lines();
    click(&mut s, 150, 150, NONE);
    drag(&mut s, (150, 150), (170, 180), NONE);
    assert_eq!(s.selected_item(), Some(b));
    assert_eq!(s.overlay().squares.len(), 2);
    s.undo();
    assert_eq!(s.selected_item(), Some(b), "still the selected item");
    assert!(s.overlay().squares.is_empty(), "the restored shape is as if deserialized");
    // It can still be picked up.
    drag(&mut s, (150, 150), (160, 150), NONE);
    assert_eq!(left_top(&s, b), (110, 150));
}

#[test]
fn multi_select_moves_together_with_one_undo() {
    let (mut s, a, b, c) = three_lines();
    click(&mut s, 150, 100, CTRL);
    click(&mut s, 150, 150, CTRL);
    s.mouse_down(150, 150, NONE);
    let ch = s.mouse_move(160, 170, NONE);
    assert_eq!(ch.scene.len(), 2);
    s.mouse_up(160, 170, NONE);
    assert_eq!(left_top(&s, a), (110, 120));
    assert_eq!(left_top(&s, b), (110, 170));
    assert_eq!(left_top(&s, c), (100, 200));
    assert_eq!(s.selected_items(), [a, b], "still selected");
    assert_eq!(s.undo_descriptions(), ["Move 2 "]);
    s.undo();
    assert_eq!(left_top(&s, a), (100, 100));
    assert_eq!(left_top(&s, b), (100, 150));
    assert_scene_fresh(&s);
}

#[test]
fn moved_shapes_are_hit_where_they_went() {
    let (mut s, _, b, _) = three_lines();
    click(&mut s, 150, 150, NONE);
    drag(&mut s, (150, 150), (150, 350), NONE);
    click(&mut s, 500, 500, NONE);
    click(&mut s, 150, 150, NONE);
    assert_eq!(s.selected_item(), None, "the old place is empty");
    click(&mut s, 150, 350, NONE);
    assert_eq!(s.selected_item(), Some(b));
}

#[test]
fn nudging_moves_by_one() {
    let (mut s, a, b, c) = three_lines();
    click(&mut s, 150, 100, NONE);
    let ch = s.key_down(Key::Right, NONE);
    assert_eq!(ch.scene.len(), 1);
    s.key_down(Key::Down, NONE);
    assert_eq!(left_top(&s, a), (101, 101));
    s.key_down(Key::Left, NONE);
    s.key_down(Key::Up, NONE);
    assert_eq!(left_top(&s, a), (100, 100));

    click(&mut s, 500, 500, NONE);
    click(&mut s, 150, 150, CTRL);
    click(&mut s, 150, 200, CTRL);
    s.key_down(Key::Down, NONE);
    assert_eq!(left_top(&s, b), (100, 151));
    assert_eq!(left_top(&s, c), (100, 201));
    assert_eq!(left_top(&s, a), (100, 100));
    assert_eq!(s.num_undoable(), 2, "each run of nudges is one Move step"); // STUDIO: designer-live-undo
    assert_scene_fresh(&s);
}

// ----- resize and rotate handles

#[test]
fn dragging_a_handle_resizes_with_undo() {
    let (mut s, _, b, _) = three_lines();
    click(&mut s, 150, 150, NONE);
    let before = lights_of(&s, b);
    drag(&mut s, (200, 150), (300, 180), NONE);
    let sh = &s.item(b).unwrap().shape;
    assert_eq!((sh.right(), sh.bottom()), (300, 180));
    assert_eq!(lights_of(&s, b)[10], (300.0, 180.0));
    assert_eq!(s.undo_descriptions(), ["Resize 1"]);
    let after = lights_of(&s, b);
    s.undo();
    assert_eq!(lights_of(&s, b), before);
    s.redo();
    assert_eq!(lights_of(&s, b), after);
}

#[test]
fn dragging_the_rotate_handle_rotates_with_undo() {
    let (mut s, _, b, _) = three_lines();
    click(&mut s, 150, 150, NONE);
    let before = lights_of(&s, b);
    // To the right of the center: 90 degrees, so the line stands up through its center.
    drag(&mut s, (150, 140), (300, 150), NONE);
    let sh = &s.item(b).unwrap().shape;
    assert_eq!(sh.rotation_angle(), 90);
    assert_eq!(lights_of(&s, b)[0], (150.0, 100.0));
    assert_eq!(lights_of(&s, b)[10], (150.0, 200.0));
    s.undo();
    assert_eq!(s.item(b).unwrap().shape.rotation_angle(), 0);
    assert_eq!(lights_of(&s, b), before);
}

#[test]
fn control_while_rotating_snaps() {
    let (mut s, _, b, _) = three_lines();
    click(&mut s, 150, 150, NONE);
    s.mouse_down(150, 140, NONE);
    s.mouse_move(230, 70, CTRL);
    s.mouse_up(230, 70, CTRL);
    assert_eq!(s.item(b).unwrap().shape.rotation_angle(), 45);
}

#[test]
fn a_rotated_shape_keeps_rotating_about_its_center_after_moving() {
    let (mut s, _, b, _) = three_lines();
    click(&mut s, 150, 150, NONE);
    drag(&mut s, (150, 140), (300, 150), NONE);
    // Grab it by a light, on the line now standing up, and carry it right.
    drag(&mut s, (150, 150), (200, 150), NONE);
    assert_eq!(lights_of(&s, b)[0], (200.0, 100.0));
    assert_eq!(lights_of(&s, b)[10], (200.0, 200.0));
}

// ----- locks and hiding

#[test]
fn a_locked_shape_needs_shift_to_select() {
    let (mut s, _, b, _) = three_lines();
    click(&mut s, 150, 150, NONE);
    let ch = s.lock_selected();
    assert!(ch.lock_hint, "the how to unlock hint, the first time");
    assert_eq!(s.selected_item(), None);
    click(&mut s, 150, 150, NONE);
    assert_eq!(s.selected_item(), None);
    click(&mut s, 150, 150, SHIFT);
    assert_eq!(s.selected_item(), Some(b));
    s.key_down(Key::U, CTRL);
    assert!(!s.item(b).unwrap().shape.locked());
    click(&mut s, 150, 150, NONE);
    let ch = s.key_down(Key::L, CTRL);
    assert!(!ch.lock_hint, "only once");
    assert!(s.item(b).unwrap().shape.locked());
    s.undo();
    assert!(!s.item(b).unwrap().shape.locked());
    s.redo();
    assert!(s.item(b).unwrap().shape.locked());
    s.key_down(Key::N, CTRL);
    assert!(!s.item(b).unwrap().shape.locked(), "unlock all");
}

#[test]
fn hiding_locked_shapes_makes_them_untouchable_and_undrawn() {
    let (mut s, _, b, _) = three_lines();
    click(&mut s, 150, 150, NONE);
    s.lock_selected();
    assert!(s.hide_locked_command_enabled());
    let runs_before: u32 = s.overlay().light_runs.iter().map(|r| r.len).sum();
    assert_eq!(runs_before, 33);
    s.set_hide_locked(true);
    let runs_after: u32 = s.overlay().light_runs.iter().map(|r| r.len).sum();
    assert_eq!(runs_after, 22, "its lights are in no run, so they are not drawn");
    click(&mut s, 150, 150, SHIFT);
    assert_eq!(s.selected_item(), None, "not even with shift");
    s.set_hide_locked(false);
    click(&mut s, 150, 150, SHIFT);
    assert_eq!(s.selected_item(), Some(b));
    s.set_hide_locked(true);
    assert_eq!(s.selected_item(), None, "hiding takes it out of the selection");
}

// ----- clipboard

#[test]
fn copy_and_paste_makes_a_new_shape_at_the_mouse() {
    let (mut s, _, b, _) = three_lines();
    click(&mut s, 150, 150, NONE);
    let ch = s.key_down(Key::C, CTRL);
    let text = ch.clipboard.expect("the clipboard text");
    assert!(text.contains("ArrayOfDisplayItem") && text.contains("PreviewLine"));
    s.mouse_move(400, 300, NONE);
    let ch = s.key_down(Key::V, CTRL);
    assert_eq!(ch.shapes_added.len(), 1);
    let new = ch.shapes_added[0];
    assert_ne!(new, b);
    assert_eq!(left_top(&s, new), (400, 300));
    assert_eq!(s.item(new).unwrap().shape.pixel_count(), 11);
    assert_eq!(lights_of(&s, new)[0], (400.0, 300.0));
    assert_eq!(s.selected_items(), [new]);
    assert_eq!(s.data().display_items.len(), 4);
    assert_scene_fresh(&s);
    // The first click after a paste ends it, and the shape becomes the selected item.
    s.mouse_down(600, 600, NONE);
    s.mouse_up(600, 600, NONE);
    assert_eq!(s.selected_item(), Some(new));
    assert_eq!(s.undo_descriptions(), ["Paste 1 "]);
    s.undo();
    assert_eq!(s.data().display_items.len(), 3);
    assert_eq!(s.scene().light_count(), 33);
    assert_eq!(s.selected_item(), None);
    s.redo();
    assert_eq!(s.data().display_items.len(), 4);
    assert_eq!(left_top(&s, new), (400, 300));
    assert_scene_fresh(&s);
}

#[test]
fn paste_keeps_element_links_and_several_shapes_keep_their_offsets() {
    let mut tree = ElementTree::new();
    let leaf = tree.add_element("Leaf");
    let leaf_id = tree.node(leaf).id;
    let tree = Arc::new(tree);
    let mut d = PreviewData::new();
    let mut line = hline(100, 100, 100);
    vixen_preview::link::link_elements(&mut line, &[leaf], &tree);
    let a = d.add_shape(line);
    let b = d.add_shape(hline(120, 150, 100));
    let mut s = DesignSession::new(d, Some(tree));
    click(&mut s, 150, 100, CTRL);
    click(&mut s, 150, 150, CTRL);
    assert_eq!(s.selected_items(), [a, b]);
    let text = s.copy().unwrap();
    s.mouse_move(400, 300, NONE);
    let ch = s.paste_text(&text);
    assert_eq!(ch.shapes_added.len(), 2);
    let (n1, n2) = (ch.shapes_added[0], ch.shapes_added[1]);
    assert_eq!(left_top(&s, n1), (400, 300));
    assert_eq!(left_top(&s, n2), (420, 350), "the top left of the group goes to the mouse");
    assert!(s.item(n1).unwrap().shape.pixels_vec().iter().all(|q| q.node_id == leaf_id));
    assert_eq!(s.scene().node_of_light(s.scene().range_of(n1).unwrap().start), Some(leaf_id));
}

#[test]
fn pasted_text_that_is_not_shapes_pastes_nothing() {
    let (mut s, _, _, _) = three_lines();
    let ch = s.paste_text("this is not xml");
    assert!(ch.shapes_added.is_empty());
    let ch = s.paste_text("<Other />");
    assert!(ch.shapes_added.is_empty());
    assert_eq!(s.data().display_items.len(), 3);
}

#[test]
fn cut_is_copy_and_delete() {
    let (mut s, _, _, _) = three_lines();
    click(&mut s, 150, 150, NONE);
    let ch = s.key_down(Key::X, CTRL);
    assert!(ch.clipboard.is_some());
    assert_eq!(ch.shapes_removed.len(), 1);
    assert_eq!(s.data().display_items.len(), 2);
    s.mouse_move(400, 300, NONE);
    s.paste();
    assert_eq!(s.data().display_items.len(), 3);
    assert_scene_fresh(&s);
}

#[test]
fn delete_with_undo_keeps_the_same_id_and_link_data() {
    let (mut s, a, b, c) = three_lines();
    click(&mut s, 150, 150, NONE);
    let before = lights_of(&s, b);
    let ch = s.key_down(Key::Delete, NONE);
    assert_eq!(ch.shapes_removed, [b]);
    // The lights after it close up.
    assert_eq!(s.scene().range_of(c), Some(11..22));
    assert_eq!(s.scene().range_of(b), None);
    assert_eq!(s.selected_item(), None);
    assert_eq!(s.undo_descriptions(), ["Removed 1 "]);
    assert_scene_fresh(&s);
    s.undo();
    // It comes back at the end of the list, as AddDisplayItem does.
    assert_eq!(s.data().display_items.iter().map(|i| i.id).collect::<Vec<_>>(), [a, c, b]);
    assert_eq!(lights_of(&s, b), before);
    assert_scene_fresh(&s);
    s.redo();
    assert_eq!(s.data().display_items.len(), 2);
    assert_scene_fresh(&s);
}

#[test]
fn delete_of_a_multi_selection() {
    let (mut s, a, b, c) = three_lines();
    click(&mut s, 150, 100, CTRL);
    click(&mut s, 150, 200, CTRL);
    s.delete_selected();
    assert_eq!(s.data().display_items.iter().map(|i| i.id).collect::<Vec<_>>(), [b]);
    assert_eq!(s.undo_descriptions(), ["Removed 2 "]);
    s.undo();
    assert_eq!(s.data().display_items.iter().map(|i| i.id).collect::<Vec<_>>(), [b, a, c]);
    assert!(s.selected_items().is_empty());
    assert_scene_fresh(&s);
}

// ----- alignment

/// Three lines of different widths at different places, selected in the order a, b, c.
fn scattered() -> (DesignSession, ShapeId, ShapeId, ShapeId) {
    let mut d = PreviewData::new();
    let a = d.add_shape(hline(100, 100, 100));
    let b = d.add_shape(hline(250, 150, 50));
    let c = d.add_shape(hline(400, 300, 20));
    let mut s = DesignSession::new(d, None);
    click(&mut s, 150, 100, CTRL);
    click(&mut s, 275, 150, CTRL);
    click(&mut s, 410, 300, CTRL);
    assert_eq!(s.selected_items(), [a, b, c]);
    (s, a, b, c)
}

#[test]
fn align_left_right_top_bottom_go_to_the_first_selected() {
    let (mut s, a, b, c) = scattered();
    s.align_left();
    assert_eq!((left_top(&s, a).0, left_top(&s, b).0, left_top(&s, c).0), (100, 100, 100));
    assert_scene_fresh(&s);
    s.undo();
    assert_eq!((left_top(&s, b).0, left_top(&s, c).0), (250, 400));
    assert_scene_fresh(&s);

    s.align_right();
    let right = |s: &DesignSession, id| s.item(id).unwrap().shape.right();
    assert_eq!((right(&s, a), right(&s, b), right(&s, c)), (200, 200, 200));
    s.undo();

    s.align_top();
    assert_eq!((left_top(&s, a).1, left_top(&s, b).1, left_top(&s, c).1), (100, 100, 100));
    s.undo();

    // Bottom: lines have no height, so they land on the first one's y.
    s.align_bottom();
    assert_eq!((left_top(&s, b).1, left_top(&s, c).1), (100, 100));
    assert_eq!(s.undo_descriptions()[0], "Move 3 ");
}

#[test]
fn align_middles_use_integer_halves() {
    let mut d = PreviewData::new();
    let a = d.add_shape(Shape::new_line(p(0, 0), p(10, 40), 3, None, 1.0));
    let b = d.add_shape(Shape::new_line(p(100, 100), p(121, 131), 3, None, 1.0));
    let mut s = DesignSession::new(d, None);
    s.select_items(&[a, b]);
    s.align_horizontal();
    // Middle of a is 0 + 40 / 2 = 20. b is 31 tall, so its top goes to 20 - 31 / 2 = 5.
    assert_eq!(left_top(&s, b).1, 5);
    s.undo();
    s.align_vertical();
    // Middle of a is 0 + 10 / 2 = 5. b is 21 wide, so its left goes to 5 - 21 / 2 = -5.
    assert_eq!(left_top(&s, b).0, -5);
}

#[test]
fn distribute_spaces_shapes_evenly() {
    let (mut s, a, b, c) = scattered();
    s.set_background_size(1000, 1000);
    s.distribute_horizontal();
    // Between a's right (200) and c's left (400) is 200, less b's width 50 is 150 to fill, in two gaps of 75.
    assert_eq!(left_top(&s, b).0, 275);
    assert_eq!((left_top(&s, a).0, left_top(&s, c).0), (100, 400));
    s.undo();
    assert_eq!(left_top(&s, b).0, 250);
    s.distribute_vertical();
    // Between a's bottom (100) and c's top (300) is 200, in two gaps of 100.
    assert_eq!(left_top(&s, b).1, 200);
    assert_scene_fresh(&s);
}

#[test]
fn distribute_needs_three_and_room_and_a_place_on_the_background() {
    let (mut s, _, b, _) = scattered();
    s.set_background_size(100, 100);
    s.distribute_horizontal();
    assert_eq!(left_top(&s, b).0, 250, "275 is off the 100 wide background");
    let mut d = PreviewData::new();
    let x = d.add_shape(hline(0, 0, 100));
    let y = d.add_shape(hline(50, 50, 100));
    let mut s = DesignSession::new(d, None);
    s.select_items(&[x, y]);
    s.distribute_horizontal();
    assert_eq!(left_top(&s, y).0, 50, "two shapes have nothing between them");
}

#[test]
fn match_properties_needs_like_shapes() {
    let mut d = PreviewData::new();
    let a = d.add_shape(hline(0, 0, 100));
    let b = d.add_shape(hline(50, 50, 10));
    let c = d.add_shape(Shape::new_single(p(5, 5), None, 1.0));
    let mut s = DesignSession::new(d, None);
    s.select_items(&[a, c]);
    let ch = s.match_properties();
    assert!(ch.message.unwrap().contains("like shapes"));
    s.clear_selection();
    s.select_items(&[a, b]);
    s.match_properties();
    assert_eq!(s.item(b).unwrap().shape.right(), 150, "b's first point stays and a's length comes across");
    s.undo();
    assert_eq!(s.item(b).unwrap().shape.right(), 60);
}

#[test]
fn alignment_touches_only_the_selected_shapes_in_the_scene() {
    let (mut s, a, b, c) = scattered();
    let ch = s.align_left();
    let ids: Vec<_> = ch.scene.iter().map(|d| d.shape).collect();
    assert_eq!(ids, [Some(b), Some(c)]);
    let _ = a;
}

// ----- groups and templates

#[test]
fn grouping_makes_one_shape_and_undo_separates_it() {
    let (mut s, a, b, _) = three_lines();
    click(&mut s, 150, 100, CTRL);
    click(&mut s, 150, 150, CTRL);
    assert_eq!(s.selected_items(), [a, b]);
    let ch = s.group_selected();
    assert_eq!(ch.shapes_removed.len(), 2);
    assert_eq!(ch.shapes_added.len(), 1);
    let group = ch.shapes_added[0];
    assert_eq!(s.selected_item(), Some(group));
    assert_eq!(s.data().display_items.len(), 2);
    assert_eq!(s.item(group).unwrap().shape.pixel_count(), 22);
    assert_eq!(s.scene().light_count(), 33);
    assert_eq!(s.item(group).unwrap().shape.type_name(), "Custom Template");
    assert_scene_fresh(&s);

    // It is selected, so it moves straight away, as one, and its children with it.
    drag(&mut s, (150, 100), (170, 110), NONE);
    assert_eq!(left_top(&s, group), (120, 110));
    let moved = s.scene().lights.clone();

    s.undo(); // the move
    s.undo(); // the group
    assert_eq!(s.data().display_items.len(), 3);
    assert_eq!(s.scene().light_count(), 33);
    assert!(s.selected_items().is_empty());
    assert_scene_fresh(&s);

    s.redo();
    assert_eq!(s.data().display_items.len(), 2);
    assert_scene_fresh(&s);
    // The second redo is the move, on a group that is no longer the one it saved: it finds nothing to swap.
    s.redo();
    assert_scene_fresh(&s);
    let _ = moved;
}

#[test]
fn ungroup_gives_the_shapes_back() {
    let (mut s, a, b, _) = three_lines();
    s.select_items(&[a, b]);
    let ch = s.group_selected();
    let group = ch.shapes_added[0];
    let ch = s.mouse_down_with(MouseButton::Right, 120, 120, NONE);
    s.mouse_up_with(MouseButton::Right, 120, 120, NONE);
    assert!(ch.context_menu.unwrap().contains(&MenuItem::Ungroup));
    assert_eq!(s.selected_item(), Some(group));
    let ch = s.ungroup_selected();
    assert_eq!(ch.shapes_added.len(), 2);
    assert_eq!(ch.shapes_removed, [group]);
    assert_eq!(s.data().display_items.len(), 3);
    assert_eq!(s.selected_item(), None);
    assert_scene_fresh(&s);
}

#[test]
fn groups_of_groups_and_custom_props_are_refused() {
    let (mut s, a, b, c) = three_lines();
    s.select_items(&[a, b]);
    let group = s.group_selected().shapes_added[0];
    s.clear_selection();
    s.select_items(&[group, c]);
    let ch = s.group_selected();
    assert!(ch.message.unwrap().contains("already grouped"));
    assert_eq!(s.data().display_items.len(), 2);

    let mut d = PreviewData::new();
    let mut prop = Shape::new_custom_prop(1.0);
    prop.add_light_nodes(&vixen_preview::shape::CustomPropLightNode { node: Uuid::nil(), light_size: 3, lights: vec![(0.0, 0.0), (10.0, 10.0)] });
    prop.layout();
    let x = d.add_shape(prop);
    let y = d.add_shape(hline(0, 0, 10));
    let mut s = DesignSession::new(d, None);
    s.select_items(&[x, y]);
    let ch = s.group_selected();
    assert!(ch.message.unwrap().contains("Custom Prop"));
}

#[test]
fn templates_save_and_add_with_links_cleared() {
    let mut tree = ElementTree::new();
    let leaf = tree.add_element("Leaf A");
    let tree = Arc::new(tree);
    let mut d = PreviewData::new();
    let mut line = hline(100, 100, 100);
    vixen_preview::link::link_elements(&mut line, &[leaf], &tree);
    let a = d.add_shape(line);
    let b = d.add_shape(hline(100, 150, 100));
    let mut s = DesignSession::new(d, Some(Arc::clone(&tree)));
    s.select_items(&[a, b]);
    let (ch, xml) = s.create_template("My Template");
    let xml = xml.expect("the template's text");
    assert!(xml.contains("DisplayItem") && xml.contains("PreviewCustom") && xml.contains("My Template"));
    assert_eq!(ch.shapes_added.len(), 1);

    let folder = std::env::temp_dir().join(format!("vixen-preview-test-{}", Uuid::new_v4()));
    let store = TemplateStore::new(&folder);
    let file = store.save("My Template", &xml).unwrap();
    assert!(store.save("a/b", &xml).is_err());
    std::fs::write(store.folder().join("broken.xml"), "not xml").unwrap();
    let (entries, errors) = store.list();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name.as_deref(), Some("My Template"));
    assert_eq!(errors.len(), 1);

    let text = store.read(&file).unwrap();
    let ch = s.add_template(&text);
    let placed = ch.shapes_added[0];
    assert_eq!(left_top(&s, placed), (10, 10));
    assert_eq!(s.selected_item(), Some(placed));
    assert_eq!(s.undo_descriptions()[0], "Added 1 ");
    assert!(s.item(placed).unwrap().shape.pixels_vec().iter().all(|q| q.node_id.is_nil()), "links are removed from a template");
    assert_scene_fresh(&s);

    store.delete(&file).unwrap();
    std::fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn a_template_links_its_named_singles_to_the_selected_nodes_leaves() {
    let mut tree = ElementTree::new();
    let group = tree.add_node(Uuid::new_v4(), "Face", None);
    let eye = tree.add_element("Face Eye Left");
    let mouth = tree.add_element("Face Mouth");
    tree.add_child(group, eye);
    tree.add_child(group, mouth);
    let (group_id, eye_id, mouth_id) = (tree.node(group).id, tree.node(eye).id, tree.node(mouth).id);
    let tree = Arc::new(tree);

    let mut s1 = Shape::new_single(p(0, 0), None, 1.0);
    s1.set_property("Name", PropertyValue::Text("Eye".to_string()), "").unwrap();
    let mut s2 = Shape::new_single(p(10, 0), None, 1.0);
    s2.set_property("Name", PropertyValue::Text("Mouth".to_string()), "").unwrap();
    let mut d = PreviewData::new();
    let a = d.add_shape(s1);
    let b = d.add_shape(s2);
    let mut s = DesignSession::new(d, Some(Arc::clone(&tree)));
    s.select_items(&[a, b]);
    let (_, xml) = s.create_template("Face");

    s.set_selected_nodes(&[group_id]);
    let placed = s.add_template(&xml.unwrap()).shapes_added[0];
    let links: Vec<Uuid> = s.item(placed).unwrap().shape.pixels_vec().iter().map(|q| q.node_id).collect();
    assert_eq!(links, [eye_id, mouth_id]);
}

// ----- zoom

#[test]
fn zoom_moves_the_lights_on_screen_and_hit_testing_follows() {
    let (mut s, _, b, _) = three_lines();
    let ch = s.set_zoom(2.0);
    assert_eq!(ch.zoom_changed, Some(2.0));
    assert_eq!(ch.scene[0].range, 0..33);
    assert_eq!(s.scene().zoom(), 2.0);
    assert_eq!(on_screen(&s, b)[0], (200.0, 300.0));
    assert_eq!(s.scene().lights[0].size, 3.0, "zoom moves lights but does not resize them");
    click(&mut s, 150, 150, NONE);
    assert_eq!(s.selected_item(), None, "the shape is at twice that now");
    click(&mut s, 300, 300, NONE);
    assert_eq!(s.selected_item(), Some(b));
    // Handles go through the renderer's view as the lights do.
    assert_eq!(s.overlay().squares[0].center, [100.0, 150.0]);
    // Back to the same place, dragging at this zoom moves half as far in the model.
    drag(&mut s, (300, 300), (320, 340), NONE);
    assert_eq!(left_top(&s, b), (110, 170));
    assert_eq!(on_screen(&s, b)[0], (220.0, 340.0));
    s.set_zoom(1.0);
    assert_eq!(lights_of(&s, b)[0], (110.0, 170.0));
    assert_scene_fresh(&s);
}

#[test]
fn through_the_view_a_light_lands_on_the_pixel_the_original_zooms_it_to() {
    // 0.75 and 1.25 aren't kind to division: 101 * 1.25 truncates to 126, which is what must come back.
    let mut d = PreviewData::new();
    let a = d.add_shape(hline(101, 103, 97));
    let mut s = DesignSession::new(d, None);
    for zoom in [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 2.75, 4.0, 1.12] {
        s.set_zoom(zoom);
        let pixels = s.item(a).unwrap().shape.pixels_vec();
        let lights = on_screen(&s, a);
        assert_eq!(pixels.len(), lights.len());
        for (p, l) in pixels.iter().zip(&lights) {
            // The renderer snaps anything within 1/256 of a whole pixel on to it.
            assert!((p.x as f32 - l.0).abs() < 1.0 / 256.0 && (p.y as f32 - l.1).abs() < 1.0 / 256.0, "{zoom}: {p:?} {l:?}");
        }
    }
    s.set_zoom(1.25);
    assert_eq!(s.item(a).unwrap().shape.pixels_vec()[0].x, 126);
}

#[test]
fn a_discrete_colour_offset_stays_in_screen_pixels_at_any_zoom() {
    let mut t = ElementTree::new();
    let rgb = t.add_element("rgb");
    t.node_mut(rgb).properties.push(PropertyInstance {
        type_id: property_ids::COLOR,
        instance_id: Uuid::new_v4(),
        data: PropertyData::Color { color_type: ElementColorType::MultipleDiscreteColors, single_color: Color::EMPTY, color_set_name: Some("RGBW".to_string()) },
    });
    let node = t.node(rgb).id;
    let mut d = PreviewData::new();
    let mut single = Shape::new_single(p(100, 200), None, 1.0);
    single.for_each_pixel_mut(&mut |p| p.node_id = node);
    single.set_pixel_size(4);
    let a = d.add_shape(single);
    let mut s = DesignSession::new(d, Some(Arc::new(t)));
    s.set_zoom(2.0);
    assert_eq!(on_screen(&s, a), [(200.0, 400.0), (204.0, 400.0), (200.0, 404.0), (204.0, 404.0)]);
    assert_scene_fresh(&s);
}

#[test]
fn the_overlay_goes_through_the_same_view_as_the_lights() {
    let (mut s, _, b, _) = three_lines();
    s.set_zoom(2.0);

    // The rubber band is where the mouse is, on screen.
    s.mouse_down(40, 60, NONE);
    s.mouse_move(100, 120, NONE);
    let band = s.overlay().rubber_band.unwrap();
    assert_eq!((band.x * 2.0, band.y * 2.0, band.width * 2.0, band.height * 2.0), (40.0, 60.0, 60.0, 60.0));
    s.mouse_up(100, 120, NONE);

    // A handle stays on its light when the shape is dragged.
    click(&mut s, 300, 300, NONE);
    drag(&mut s, (300, 300), (333, 351), NONE);
    let handle = s.overlay().squares[0].center;
    let light = on_screen(&s, b)[0];
    assert_eq!((handle[0] * 2.0, handle[1] * 2.0), light);
}

#[test]
fn a_drag_moves_lights_without_changing_their_colours() {
    let (mut s, _, b, _) = three_lines();
    assert!(s.mouse_down(150, 150, NONE).light_colors_changed, "it's selected now");
    s.mouse_up(150, 150, NONE);
    s.mouse_down(150, 150, NONE);
    let ch = s.mouse_move(170, 180, NONE);
    assert_eq!(ch.scene[0].range, s.scene().range_of(b).unwrap());
    assert!(ch.overlay_changed, "the handles moved");
    assert!(!ch.light_colors_changed);
    s.mouse_up(170, 180, NONE);
    assert!(!s.mouse_move(500, 500, NONE).light_colors_changed);
    assert!(s.mouse_down(500, 500, NONE).light_colors_changed, "deselected");
}

#[test]
fn the_preview_settings_go_back_into_the_saved_data() {
    let (mut s, _, _, _) = three_lines();
    s.set_keep_insert_mode(true);
    s.set_save_locations(true);
    s.set_use_open_gl(true);
    s.set_location_offset(1.5, -2.0, 3.0);
    s.set_background_file(Some("back.png".to_string()));
    s.set_setup_bounds(1, 2, 3, 4);
    s.set_display_bounds(5, 6, 7, 8);
    let d = PreviewData::from_xml(&s.data().to_xml()).unwrap();
    assert!(d.keep_insert_mode && d.save_locations && d.use_open_gl);
    assert_eq!((d.location_offset.x.value, d.location_offset.y.value, d.location_offset.z.value), (1.5, -2.0, 3.0));
    assert_eq!(d.background_file_name.as_deref(), Some("back.png"));
    assert_eq!((d.setup_left, d.setup_top, d.setup_width, d.setup_height), (1, 2, 3, 4));
    assert_eq!((d.left, d.top, d.width, d.height), (5, 6, 7, 8));
    s.set_background_file(None);
    assert_eq!(PreviewData::from_xml(&s.data().to_xml()).unwrap().background_file_name, None);
}

#[test]
fn scrolling_is_only_a_view_change() {
    let (mut s, a, _, _) = three_lines();
    s.set_background_size(2000, 2000);
    s.set_view_size(500, 500);
    let before = s.scene().lights.clone();
    s.set_scroll(30, 40);
    assert_eq!(s.scroll(), (30, 40));
    assert_eq!(s.scene().lights, before);
    // A light at (x, y) shows at (x, y) minus the scroll, so that is where the mouse finds it.
    let (x, y) = lights_of(&s, a)[0];
    click(&mut s, x as i32 - 30, y as i32 - 40, NONE);
    assert_eq!(s.selected_item(), Some(a));
}

#[test]
fn zoom_is_clamped_and_has_keys_and_a_wheel() {
    let (mut s, _, _, _) = three_lines();
    assert_eq!(s.set_zoom(100.0).zoom_changed, Some(4.0));
    assert_eq!(s.set_zoom(0.0).zoom_changed, Some(0.25));
    s.set_zoom(1.0);
    assert_eq!(s.key_down(Key::Plus, CTRL).zoom_changed, Some(1.25));
    assert_eq!(s.key_down(Key::Minus, CTRL).zoom_changed, Some(1.0));
    assert_eq!(s.mouse_wheel(120, 50, 50).zoom_changed, Some(1.12));
    s.set_zoom(4.0);
    assert_eq!(s.key_down(Key::Plus, CTRL).zoom_changed, None, "already at the limit");
}

#[test]
fn undo_restores_at_the_current_zoom() {
    let (mut s, _, b, _) = three_lines();
    click(&mut s, 150, 150, NONE);
    drag(&mut s, (150, 150), (170, 180), NONE);
    s.set_zoom(2.0);
    s.undo();
    assert_eq!(on_screen(&s, b)[0], (200.0, 300.0));
    assert_scene_fresh(&s);
}

#[test]
fn scrolling_offsets_the_mouse() {
    let (mut s, _, b, _) = three_lines();
    s.set_scroll(100, 100);
    click(&mut s, 50, 50, NONE);
    assert_eq!(s.selected_item(), Some(b), "(50, 50) in the view is (150, 150) in the scene");
}

#[test]
fn the_middle_button_pans_when_there_is_room() {
    let (mut s, _, _, _) = three_lines();
    s.set_view_size(500, 500);
    s.set_background_size(1000, 1000);
    assert_eq!(s.scroll(), (0, 0));
    s.mouse_down_with(MouseButton::Middle, 400, 400, NONE);
    let ch = s.mouse_move(100, 150, NONE);
    assert!(ch.scroll_changed);
    assert_eq!(s.scroll(), (300, 250), "the point grabbed stays under the mouse");
    s.mouse_up_with(MouseButton::Middle, 100, 150, NONE);
    let ch = s.mouse_move(50, 50, NONE);
    assert!(!ch.scroll_changed);
}

// ----- drawing new shapes

#[test]
fn every_tool_draws_its_shape_with_the_mouse() {
    for (tool, type_name) in [
        (Tool::String, "Line"),
        (Tool::Arch, "Arch"),
        (Tool::Rectangle, "Rectangle"),
        (Tool::Ellipse, "Ellipse"),
        (Tool::Single, "Single"),
        (Tool::Triangle, "Triangle"),
        (Tool::MegaTree, "Mega Tree"),
        (Tool::Net, "Net"),
        (Tool::Flood, "Flood"),
        (Tool::Star, "Star"),
        (Tool::Cane, "Cane"),
        (Tool::PixelGrid, "Pixel Grid"),
        (Tool::StarBurst, "Star Burst"),
        (Tool::Icicle, "Icicle"),
        (Tool::MovingHead, "Intelligent Fixture"),
    ] {
        let mut s = DesignSession::new(PreviewData::new(), None);
        s.set_tool(tool);
        assert_eq!(s.tool(), tool);
        let ch = s.mouse_down(10, 10, NONE);
        assert_eq!(ch.shapes_added.len(), 1, "{type_name}");
        let id = ch.shapes_added[0];
        assert_eq!(s.item(id).unwrap().shape.type_name(), type_name);
        let ch = s.mouse_move(110, 110, NONE);
        assert_eq!(ch.scene.first().and_then(|d| d.shape), Some(id), "{type_name}");
        let ch = s.mouse_up(110, 110, NONE);
        // No element selected, so the original offers a template, for the tools that have one.
        let has_template = !matches!(tool, Tool::Single | Tool::Flood | Tool::Net);
        assert_eq!(ch.element_template.is_some(), has_template, "{type_name}");
        assert_eq!(s.tool(), Tool::Select, "{type_name} drops back to Select");
        assert_eq!(s.undo_descriptions(), ["Added 1 "], "{type_name}");
        assert_eq!(s.selected_item(), None, "{type_name}");
        assert_scene_fresh(&s);
        // Undo and redo the addition.
        s.undo();
        assert!(s.data().display_items.is_empty(), "{type_name}");
        assert_eq!(s.scene().light_count(), 0);
        s.redo();
        assert_eq!(s.data().display_items.len(), 1, "{type_name}");
        assert_scene_fresh(&s);
    }
}

#[test]
fn keep_insert_mode_stays_on_the_tool() {
    let mut d = PreviewData::new();
    d.keep_insert_mode = true;
    let mut s = DesignSession::new(d, None);
    s.set_tool(Tool::Arch);
    drag(&mut s, (10, 10), (50, 50), NONE);
    assert_eq!(s.tool(), Tool::Arch);
    drag(&mut s, (100, 100), (150, 150), NONE);
    assert_eq!(s.data().display_items.len(), 2);
    assert_scene_fresh(&s);
}

#[test]
fn a_poly_line_takes_points_until_escape() {
    let mut s = DesignSession::new(PreviewData::new(), None);
    s.set_tool(Tool::PolyLine);
    let id = s.mouse_down(0, 0, NONE).shapes_added[0];
    s.mouse_up(0, 0, NONE);
    assert!(s.item(id).unwrap().shape.is_creating());
    s.mouse_move(100, 0, NONE);
    s.mouse_down(100, 0, NONE);
    s.mouse_up(100, 0, NONE);
    s.mouse_move(100, 50, NONE);
    s.mouse_down(100, 50, NONE);
    s.mouse_up(100, 50, NONE);
    s.mouse_move(300, 300, NONE);
    let ch = s.key_down(Key::Escape, NONE);
    assert!(ch.show_properties.is_some() || ch.clear_properties);
    let sh = &s.item(id).unwrap().shape;
    assert!(!sh.is_creating());
    // Escape takes away the last point, the one following the mouse.
    assert_eq!(sh.points().iter().map(|q| (q.x, q.y)).collect::<Vec<_>>(), [(0, 0), (100, 0), (100, 50)]);
    // 150 long at 10 apart.
    assert_eq!(sh.pixel_count(), 15);
    assert!(!s.mouse_captured());
    assert_eq!(s.undo_descriptions(), ["Added 1 "]);
    assert_scene_fresh(&s);
}

#[test]
fn a_multi_string_takes_points_until_escape() {
    let mut s = DesignSession::new(PreviewData::new(), None);
    s.set_tool(Tool::MultiString);
    let id = s.mouse_down(0, 0, NONE).shapes_added[0];
    s.mouse_up(60, 0, NONE);
    s.mouse_move(60, 0, NONE);
    s.mouse_down(60, 0, NONE);
    s.mouse_up(60, 40, NONE);
    s.mouse_move(60, 40, NONE);
    s.mouse_down(60, 40, NONE);
    s.mouse_up(90, 90, NONE);
    s.key_down(Key::Escape, NONE);
    let sh = &s.item(id).unwrap().shape;
    assert!(!sh.is_creating());
    assert_eq!(sh.strings().unwrap().len(), 2);
    assert_eq!(sh.pixel_count(), 20, "ten lights a string");
    assert_eq!(sh.points().len(), 3);
    assert_scene_fresh(&s);
}

#[test]
fn escape_otherwise_goes_back_to_the_select_tool() {
    let mut s = DesignSession::new(PreviewData::new(), None);
    s.set_tool(Tool::Arch);
    let ch = s.key_down(Key::Escape, NONE);
    assert!(ch.tool_changed);
    assert_eq!(s.tool(), Tool::Select);
}

#[test]
fn add_shape_places_one_of_each_without_changing_the_tool() {
    let mut s = DesignSession::new(PreviewData::new(), None);
    for tool in [Tool::String, Tool::Arch, Tool::Rectangle, Tool::Ellipse, Tool::Single, Tool::Triangle, Tool::MegaTree, Tool::Net, Tool::Star, Tool::Cane, Tool::PixelGrid, Tool::StarBurst, Tool::Icicle, Tool::PolyLine, Tool::MultiString, Tool::MovingHead] {
        let before = s.data().display_items.len();
        let ch = s.add_shape(tool);
        assert_eq!(s.data().display_items.len(), before + 1, "{tool:?}");
        assert_eq!(ch.shapes_added.len(), 1, "{tool:?}");
        assert_eq!(s.tool(), Tool::Select);
        assert!(!s.mouse_captured(), "{tool:?}");
        assert!(s.data().display_items.last().is_some_and(|i| i.shape.right() >= 10));
        assert_scene_fresh(&s);
    }
    // They all save and load, in the XML form Vixen reads.
    let e = s.data().to_xml();
    let back = PreviewData::from_xml(&e).unwrap();
    assert_eq!(e.first_difference(&back.to_xml(), String::new()), None);
    assert_eq!(back.display_items.len(), 16);
}

#[test]
fn drawing_with_an_element_selected_links_it_and_highlights_it() {
    let mut tree = ElementTree::new();
    let group = tree.add_string("Arch 1", 30);
    let (group_id, first) = (tree.node(group).id, tree.node(tree.node(group).children[0]).id);
    let tree = Arc::new(tree);
    let mut s = DesignSession::new(PreviewData::new(), Some(tree));
    s.set_selected_nodes(&[group_id]);
    assert!(s.highlighted_elements().contains(&group_id));
    assert!(s.highlighted_elements().contains(&first));
    s.set_tool(Tool::Arch);
    let id = s.mouse_down(10, 10, NONE).shapes_added[0];
    let sh = &s.item(id).unwrap().shape;
    assert_eq!(sh.string_type(), StringType::Pixel);
    assert_eq!(sh.pixel_count(), 30);
    assert_eq!(sh.pixels_vec()[0].node_id, first);
    let ch = s.mouse_move(110, 60, NONE);
    assert!(!ch.scene.is_empty());
    let ch = s.mouse_up(110, 60, NONE);
    assert!(ch.element_template.is_none(), "it already has elements");
    assert_eq!(s.tool(), Tool::Arch, "the tool stays when an element was selected");
    assert_eq!(s.scene().node_of_light(0), Some(first));
}

#[test]
fn an_element_template_links_a_drawn_shape_afterwards() {
    let mut tree = ElementTree::new();
    let group = tree.add_string("Arch", 12);
    let id = tree.node(group).id;
    let tree = Arc::new(tree);
    let mut s = DesignSession::new(PreviewData::new(), Some(Arc::clone(&tree)));
    s.set_tool(Tool::Arch);
    s.mouse_down(10, 10, NONE);
    s.mouse_move(110, 60, NONE);
    let ch = s.mouse_up(110, 60, NONE);
    let (shape, template) = ch.element_template.unwrap();
    assert_eq!(template, "Arch");
    s.apply_element_template(shape, &[id]);
    let sh = &s.item(shape).unwrap().shape;
    assert_eq!(sh.pixel_count(), 12);
    assert_eq!(s.scene().light_count(), 12);
    assert_scene_fresh(&s);
}

#[test]
fn a_finished_template_brings_its_tree_links_the_shape_and_selects_what_it_made() {
    // The session opens on the tree as it was, and the wizard's elements arrive in a new one.
    let mut s = DesignSession::new(PreviewData::new(), Some(Arc::new(ElementTree::new())));
    s.set_tool(Tool::Arch);
    s.mouse_down(10, 10, NONE);
    s.mouse_move(110, 60, NONE);
    let ch = s.mouse_up(110, 60, NONE);
    let (shape, _) = ch.element_template.unwrap();
    assert_eq!(s.tool(), Tool::Select, "keep insert mode is off");

    let mut tree = ElementTree::new();
    let group = tree.add_string("Arch", 12);
    let id = tree.node(group).id;
    let ch = s.element_template_finished(Arc::new(tree), Some(shape), &[id], &[id]);
    assert!(ch.scene.iter().any(|d| d.resized));
    assert_eq!(s.item(shape).unwrap().shape.pixel_count(), 12);
    assert_eq!(s.scene().light_count(), 12);
    assert!(s.scene().node_of_light(0).is_some());
    assert_eq!(run_color_of_light(&s, 1).unwrap().0, theme::ELEMENT_SELECTED, "the tree's selection highlights what it's linked to");
    assert_scene_fresh(&s);

    // The Elements panel's Add button: only the tree and its selection.
    let mut tree = ElementTree::new();
    tree.add_string("Arch", 12);
    let other = tree.add_element("Bulb");
    let other_id = tree.node(other).id;
    s.element_template_finished(Arc::new(tree), None, &[], &[other_id]);
    assert_eq!(s.item(shape).unwrap().shape.pixel_count(), 12);
    assert_scene_fresh(&s);
}

#[test]
fn several_fixtures_from_one_template_make_a_shape_each() {
    let mut s = DesignSession::new(PreviewData::new(), Some(Arc::new(ElementTree::new())));
    s.set_view_size(400, 300);
    s.set_tool(Tool::MovingHead);
    s.mouse_down(10, 10, NONE);
    s.mouse_move(110, 110, NONE);
    let ch = s.mouse_up(110, 110, NONE);
    let (shape, template) = ch.element_template.unwrap();
    assert_eq!(template, "Intelligent Fixture");
    let mut tree = ElementTree::new();
    let heads: Vec<_> = (1..=3).map(|i| tree.add_element(&format!("Head {i}"))).collect();
    let ids: Vec<Uuid> = heads.iter().map(|n| tree.node(*n).id).collect();
    let before = s.data().display_items.len();
    let ch = s.element_template_finished(Arc::new(tree), Some(shape), &ids, &ids[..1]);
    assert_eq!(ch.shapes_added.len(), 2);
    assert_eq!(s.data().display_items.len(), before + 2);
    let linked: Vec<Uuid> = s
        .data()
        .display_items
        .iter()
        .filter_map(|i| match i.shape.kind() {
            vixen_preview::shape::ShapeKind::MovingHead(m) => Some(m.node_id),
            _ => None,
        })
        .collect();
    assert_eq!(linked, ids, "each clone takes the next fixture");
}

#[test]
fn a_replaced_tree_drops_links_to_what_it_no_longer_has() {
    let (tree, group) = tree_with_leaves();
    let mut d = PreviewData::new();
    let mut line = hline(100, 100, 100);
    line.set_string_type(StringType::Pixel);
    let a = d.add_shape(line);
    let mut s = DesignSession::new(d, Some(Arc::clone(&tree)));
    let mut linker = ElementLinker::new(&s.item(a).unwrap().shape);
    linker.drop_nodes(0, 0, &[group], &tree);
    s.edit_shape(a, |sh| linker.apply(sh));
    let gone = tree.node(tree.node(group).children[3]).id;
    let stays = tree.node(tree.node(group).children[4]).id;
    s.set_selected_nodes(&[gone, stays]);

    let mut edited = ElementTree::new();
    let parent = edited.add_node(tree.node(group).id, "Str", None);
    for &leaf in &tree.node(group).children {
        let n = tree.node(leaf);
        if n.id != gone {
            let copy = edited.add_node(n.id, &n.name, n.element_id);
            edited.add_child(parent, copy);
        }
    }
    s.tree_replaced(Arc::new(edited));
    assert_eq!(s.scene().node_of_light(3), None);
    assert_eq!(s.scene().node_of_light(4), Some(stays));
    assert_eq!(run_color_of_light(&s, 4).unwrap().0, theme::ELEMENT_SELECTED);
    assert_scene_fresh(&s);
}

#[test]
fn single_tool_names_and_sizes_as_set_up() {
    let mut s = DesignSession::new(PreviewData::new(), None);
    s.set_tool(Tool::Single);
    s.set_single_item_defaults("Bulb", 5, 7);
    s.data();
    let a = s.mouse_down(10, 10, NONE).shapes_added[0];
    s.mouse_up(10, 10, NONE);
    s.set_tool(Tool::Single);
    s.set_single_item_defaults("Bulb", 6, 7);
    let b = s.mouse_down(30, 10, NONE).shapes_added[0];
    assert_eq!(s.item(a).unwrap().shape.name(), Some("Bulb5"));
    assert_eq!(s.item(a).unwrap().shape.pixel_size(), 7);
    assert_eq!(s.item(b).unwrap().shape.name(), Some("Bulb6"));
}

// ----- colours

fn run_color_of_light(s: &DesignSession, light: u32) -> Option<(Color, f32)> {
    s.overlay().light_runs.iter().find(|r| light >= r.start && light < r.start + r.len).map(|r| (r.color, r.size_delta))
}

fn tree_with_leaves() -> (Arc<ElementTree>, vixen_core::NodeId) {
    let mut t = ElementTree::new();
    let g = t.add_string("Str", 11);
    (Arc::new(t), g)
}

#[test]
fn link_elements_on_the_session_reports_the_new_links() {
    let (tree, group) = tree_with_leaves();
    let mut d = PreviewData::new();
    let mut line = hline(100, 100, 100);
    line.set_string_type(StringType::Pixel);
    let a = d.add_shape(line);
    let mut s = DesignSession::new(d, Some(Arc::clone(&tree)));

    let ch = s.link_elements(a, &[group]);
    assert!(ch.scene[0].links_changed);
    for i in 0..11 {
        assert_eq!(s.scene().node_of_light(i), Some(tree.node(tree.node(group).children[i]).id));
    }
    assert_eq!(s.scene().node_of_light(11), None);
    assert_scene_fresh(&s);

    // With no element tree there is nothing to link to, and nothing happens.
    let mut d = PreviewData::new();
    let a = d.add_shape(hline(100, 100, 100));
    let mut s = DesignSession::new(d, None);
    assert!(s.link_elements(a, &[group]).scene.is_empty());
    assert_eq!(s.scene().node_of_light(0), None);
}

#[test]
fn edit_colours_are_the_original_theme() {
    assert_eq!(theme::SELECTED, Color::rgb(50, 205, 50));
    assert_eq!(theme::LINKED, Color::rgb(64, 224, 208));
    assert_eq!(theme::UNLINKED, Color::WHITE);
    assert_eq!(theme::ELEMENT_SELECTED, Color::rgb(255, 105, 180));
    assert_eq!(theme::LOCKED, Color::rgb(139, 0, 0));
    assert_eq!(theme::PIXEL1, Color::YELLOW);
}

#[test]
fn lights_take_their_edit_colour_from_state() {
    let (tree, group) = tree_with_leaves();
    let group_id = tree.node(group).id;
    let mut d = PreviewData::new();
    let mut linked = hline(100, 100, 100);
    vixen_preview::link::link_elements(&mut linked, &[tree.node(group).children[0]], &tree);
    let a = d.add_shape(linked);
    let b = d.add_shape(hline(100, 150, 100));
    let mut s = DesignSession::new(d, Some(Arc::clone(&tree)));
    let (ra, rb) = (s.scene().range_of(a).unwrap().start as u32, s.scene().range_of(b).unwrap().start as u32);

    assert_eq!(run_color_of_light(&s, ra), Some((theme::LINKED, 0.0)));
    assert_eq!(run_color_of_light(&s, rb), Some((theme::UNLINKED, 0.0)));

    // Selected wins over linked.
    click(&mut s, 150, 100, NONE);
    assert_eq!(run_color_of_light(&s, ra), Some((theme::SELECTED, 0.0)));
    click(&mut s, 500, 500, NONE);
    assert_eq!(run_color_of_light(&s, ra), Some((theme::LINKED, 0.0)));

    // So does multi-selection. One shape multi-selected stays so until control-clicked again: the C# only clears
    // on a click elsewhere when there is more than one.
    click(&mut s, 150, 150, CTRL);
    assert_eq!(run_color_of_light(&s, rb), Some((theme::SELECTED, 0.0)));
    click(&mut s, 500, 500, NONE);
    assert_eq!(run_color_of_light(&s, rb), Some((theme::SELECTED, 0.0)));
    click(&mut s, 150, 150, CTRL);
    assert_eq!(run_color_of_light(&s, rb), Some((theme::UNLINKED, 0.0)));

    // A selected element highlights lights linked to it or anything under it.
    let ch = s.set_selected_nodes(&[group_id]);
    assert!(ch.overlay_changed);
    assert_eq!(run_color_of_light(&s, ra), Some((theme::ELEMENT_SELECTED, 0.0)));
    assert_eq!(s.overlay().element_highlights().map(|r| r.len).sum::<u32>(), 11);
    assert_eq!(run_color_of_light(&s, rb), Some((theme::UNLINKED, 0.0)));
    s.set_selected_nodes(&[]);
    assert_eq!(run_color_of_light(&s, ra), Some((theme::LINKED, 0.0)));

    // A locked shape's linked lights are dark red, and its unlinked ones stay white.
    s.edit_shape(a, |sh| sh.set_locked(true));
    s.edit_shape(b, |sh| sh.set_locked(true));
    assert_eq!(run_color_of_light(&s, ra), Some((theme::LOCKED, 0.0)));
    assert_eq!(run_color_of_light(&s, rb), Some((theme::UNLINKED, 0.0)));
}

#[test]
fn the_first_pixel_of_a_pixel_string_is_yellow_and_bigger() {
    let (tree, group) = tree_with_leaves();
    let mut d = PreviewData::new();
    let mut line = hline(100, 100, 100);
    line.set_string_type(StringType::Pixel);
    vixen_preview::link::link_elements(&mut line, &[group], &tree);
    let a = d.add_shape(line);
    let mut s = DesignSession::new(d, Some(tree));
    let o = s.overlay();
    assert_eq!(o.light_runs.len(), 2, "the runs coalesce: one yellow, then the rest");
    assert_eq!((o.light_runs[0].color, o.light_runs[0].size_delta, o.light_runs[0].len), (theme::PIXEL1, 2.0, 1));
    assert_eq!((o.light_runs[1].color, o.light_runs[1].len), (theme::LINKED, 10));
    // Even when selected, and not when locked.
    click(&mut s, 150, 100, NONE);
    assert_eq!(s.overlay().light_runs[0].color, theme::PIXEL1);
    assert_eq!(s.overlay().light_runs[1].color, theme::SELECTED);
    s.lock_selected();
    assert_eq!(s.overlay().light_runs[0].color, theme::LOCKED);
    assert_eq!(s.overlay().light_runs.len(), 1);
    let _ = a;
}

#[test]
fn a_rectangles_first_pixel_is_only_the_first_strings() {
    let mut d = PreviewData::new();
    let mut r = Shape::new_rectangle(p(0, 0), None, 1.0);
    r.set_string_type(StringType::Pixel);
    r.set_property("String 1 Light Count", PropertyValue::Int(4), "").unwrap();
    d.add_shape(r);
    let s = DesignSession::new(d, None);
    let yellow: u32 = s.overlay().light_runs.iter().filter(|x| x.color == theme::PIXEL1).map(|x| x.len).sum();
    assert_eq!(yellow, 1);
}

#[test]
fn a_groups_strings_each_have_their_first_pixel() {
    let mut d = PreviewData::new();
    let mut l1 = hline(0, 0, 10);
    l1.set_string_type(StringType::Pixel);
    let mut l2 = hline(0, 10, 10);
    l2.set_string_type(StringType::Pixel);
    d.add_shape(Shape::new_custom(p(0, 0), vec![l1, l2]));
    let s = DesignSession::new(d, None);
    let yellow: u32 = s.overlay().light_runs.iter().filter(|x| x.color == theme::PIXEL1).map(|x| x.len).sum();
    assert_eq!(yellow, 2);
}

#[test]
fn colours_can_fill_a_renderer_buffer() {
    let (mut s, _, _, _) = three_lines();
    click(&mut s, 150, 150, NONE);
    let mut out = vec![0u32; s.scene().light_count()];
    s.overlay().fill_light_colors(&mut out, 0, |c| c.to_argb_u32());
    assert_eq!(out[0], theme::UNLINKED.to_argb_u32());
    assert_eq!(out[11], theme::SELECTED.to_argb_u32());
    assert_eq!(out[22], theme::UNLINKED.to_argb_u32());
}

#[test]
fn overlay_has_fixtures_and_show_info() {
    let mut d = PreviewData::new();
    let mut mh = Shape::new_moving_head(p(10, 20), None, 1.0);
    mh.select(true);
    mh.select_default_select_point();
    mh.mouse_move(60, 70, 50, 50, NONE);
    mh.end_add_new();
    mh.deselect();
    let id = d.add_shape(mh);
    let l = d.add_shape(hline(100, 100, 50));
    let mut s = DesignSession::new(d, None);
    let o = s.overlay();
    assert_eq!(o.fixtures.len(), 1);
    assert_eq!((o.fixtures[0].x, o.fixtures[0].y, o.fixtures[0].width, o.fixtures[0].height), (10.0, 20.0, 50.0, 50.0));
    assert_eq!(o.fixtures[0].beam_color, theme::UNLINKED);
    assert!(o.lines.is_empty());
    click(&mut s, 30, 40, NONE);
    assert_eq!(s.selected_item(), Some(id));
    let o = s.overlay();
    assert_eq!(o.fixtures[0].beam_color, theme::SELECTED);
    assert_eq!(o.lines.len(), 4, "the selection outline");
    assert_eq!(o.lines[0].color, theme::FIXTURE_OUTLINE);
    assert_eq!(s.scene().fixtures[0].left, 10);
    drag(&mut s, (30, 40), (50, 60), NONE);
    assert_eq!(s.scene().fixtures[0].left, 30, "the scene's fixture moves with it");

    s.set_show_info(true);
    click(&mut s, 500, 500, NONE);
    let o = s.overlay();
    assert_eq!(o.labels.len(), 1, "only light shapes have the Z label");
    assert_eq!(o.labels[0].text, "Z:0");
    assert_eq!(o.lines.len(), 4);
    assert_eq!(o.lines[0].color, theme::INFO_OUTLINE);
    let _ = l;
}

// ----- properties, links, bulb size, Z

#[test]
fn property_edits_through_the_session_update_the_scene() {
    let (mut s, a, b, _) = three_lines();
    let rows = s.shape_properties(a);
    assert!(rows.iter().any(|r| r.name == "Light Count" && r.category == "Settings" && r.value == PropertyValue::Int(11)));
    assert!(s.shape_properties(ShapeId(9999)).is_empty());

    let ch = s.set_shape_property(a, "Light Count", PropertyValue::Int(5)).unwrap();
    assert!(ch.scene[0].resized);
    assert_eq!(s.scene().light_count(), 27);
    assert_eq!(s.scene().range_of(b), Some(5..16));
    assert_scene_fresh(&s);

    let ch = s.set_shape_property(a, "Point 2", PropertyValue::Point { x: 300, y: 100 }).unwrap();
    assert!(!ch.scene.is_empty());
    assert_eq!(lights_of(&s, a)[4], (300.0, 100.0));
    assert_scene_fresh(&s);

    assert_eq!(s.set_shape_property(a, "Nope", PropertyValue::Int(1)).err(), Some(PropertyError::UnknownProperty));
    assert_eq!(s.set_shape_property(ShapeId(9999), "Name", PropertyValue::Int(1)).err(), Some(PropertyError::UnknownShape));
}

#[test]
fn a_new_name_falls_back_to_the_tool_name_when_emptied() {
    let mut s = DesignSession::new(PreviewData::new(), None);
    s.set_tool(Tool::Cane);
    let id = s.mouse_down(10, 10, NONE).shapes_added[0];
    s.mouse_move(50, 80, NONE);
    s.mouse_up(50, 80, NONE);
    s.set_shape_property(id, "Name", PropertyValue::Text(String::new())).unwrap();
    assert_eq!(s.item(id).unwrap().shape.name(), Some("Candy Cane"));
}

#[test]
fn linking_through_the_session_changes_what_lights_show() {
    let (tree, group) = tree_with_leaves();
    let mut d = PreviewData::new();
    let mut line = hline(100, 100, 100);
    line.set_string_type(StringType::Pixel);
    let a = d.add_shape(line);
    let mut s = DesignSession::new(d, Some(Arc::clone(&tree)));
    assert_eq!(s.scene().node_of_light(3), None);
    let mut linker = ElementLinker::new(&s.item(a).unwrap().shape);
    linker.drop_nodes(0, 0, &[group], &tree);
    let ch = s.edit_shape(a, |sh| linker.apply(sh));
    assert!(ch.scene[0].links_changed);
    assert_eq!(s.scene().node_of_light(3), Some(tree.node(tree.node(group).children[3]).id));
    assert_eq!(run_color_of_light(&s, 3).unwrap().0, theme::LINKED);
    assert_scene_fresh(&s);

    // Deleting the element removes the links.
    let leaf = tree.node(tree.node(group).children[3]).id;
    s.unlink_nodes(&[leaf]);
    assert_eq!(s.scene().node_of_light(3), None);
    assert_eq!(s.scene().node_of_light(4), Some(tree.node(tree.node(group).children[4]).id));
}

#[test]
fn discrete_lights_follow_their_shape() {
    let mut t = ElementTree::new();
    let n = t.add_element("RGB bulb");
    t.node_mut(n).properties.push(PropertyInstance {
        type_id: property_ids::COLOR,
        instance_id: Uuid::new_v4(),
        data: PropertyData::Color { color_type: ElementColorType::MultipleDiscreteColors, single_color: Color::EMPTY, color_set_name: Some("RGB".to_string()) },
    });
    let t = Arc::new(t);
    let mut d = PreviewData::new();
    let mut line = hline(100, 100, 100);
    vixen_preview::link::link_elements(&mut line, &[n], &t);
    let a = d.add_shape(line);
    let b = d.add_shape(hline(100, 150, 100));
    let mut s = DesignSession::new(d, Some(t));
    assert_eq!(s.scene().light_count(), 33 + 11);
    assert_eq!(s.scene().range_of(b), Some(33..44));
    click(&mut s, 150, 100, NONE);
    let ch = drag(&mut s, (150, 100), (160, 110), NONE);
    assert_eq!(ch.scene[0].range, 0..33);
    assert_eq!(lights_of(&s, a)[..3], [(110.0, 110.0), (113.0, 110.0), (110.0, 113.0)]);
    assert_eq!(s.overlay().light_runs[0].len, 33, "all three lights of each pixel take its colour");
    assert_scene_fresh(&s);
}

#[test]
fn bulb_size_changes_with_undo() {
    let (mut s, a, b, _) = three_lines();
    click(&mut s, 150, 100, NONE);
    let ch = s.increase_bulb_size();
    assert_eq!(ch.scene[0].shape, Some(a));
    assert_eq!(s.scene().lights[0].size, 4.0);
    assert_eq!(s.undo_descriptions()[0], "Pixel size on 1 items.");
    s.undo();
    assert_eq!(s.scene().lights[0].size, 3.0);
    s.redo();
    assert_eq!(s.scene().lights[0].size, 4.0);

    click(&mut s, 500, 500, NONE);
    click(&mut s, 150, 100, CTRL);
    click(&mut s, 150, 150, CTRL);
    s.decrease_bulb_size();
    assert_eq!(s.item(a).unwrap().shape.pixel_size(), 3);
    assert_eq!(s.item(b).unwrap().shape.pixel_size(), 2);
    assert_scene_fresh(&s);

    s.match_bulb_size();
    assert_eq!(s.item(b).unwrap().shape.pixel_size(), 3);
    assert_scene_fresh(&s);
}

#[test]
fn z_and_location_updates() {
    let (tree, group) = tree_with_leaves();
    let leaf = tree.node(group).children[0];
    let leaf_id = tree.node(leaf).id;
    let mut d = PreviewData::new();
    let mut line = hline(100, 100, 100);
    vixen_preview::link::link_elements(&mut line, &[leaf], &tree);
    let a = d.add_shape(line);
    let mut s = DesignSession::new(d, Some(Arc::clone(&tree)));
    click(&mut s, 150, 100, NONE);
    let ch = s.set_z(4);
    assert_eq!(s.item(a).unwrap().shape.pixels_vec()[0].z, 4);
    assert_eq!(ch.location_updates.len(), 11);
    assert_eq!(ch.location_updates[0], vixen_preview::session::LocationUpdate { node: leaf_id, x: 100, y: 4 }, "Y gets the Z, as in the C#");

    let updates = s.location_updates();
    assert_eq!(updates.len(), 11);
    assert_eq!((updates[0].x, updates[0].y), (100, 100));
    let mut t = (*tree).clone();
    apply_location_updates(&mut t, &updates);
    assert_eq!(t.location(leaf), (200, 100), "every pixel writes, and the last pixel's location wins");

    s.set_shape_property(a, "Common Location", PropertyValue::Bool(true)).unwrap();
    assert_eq!((s.location_updates()[0].x, s.location_updates()[0].y), (150, 100), "the center, for all of them");

    let mut d = PreviewData::new();
    d.save_locations = false;
    assert!(DesignSession::new(d, None).location_updates().is_empty());
}

// ----- context menu

#[test]
fn the_right_button_menu_matches_the_state() {
    let (mut s, a, _, _) = three_lines();
    let ch = s.mouse_down_with(MouseButton::Right, 150, 100, NONE);
    s.mouse_up_with(MouseButton::Right, 150, 100, NONE);
    assert_eq!(s.selected_item(), Some(a));
    let menu = ch.context_menu.unwrap();
    assert_eq!(menu, [MenuItem::Cut, MenuItem::Copy, MenuItem::Delete, MenuItem::Lock, MenuItem::SetZ]);

    s.copy();
    s.lock_selected();
    let ch = s.mouse_down_with(MouseButton::Right, 500, 500, NONE);
    s.mouse_up_with(MouseButton::Right, 500, 500, NONE);
    assert_eq!(ch.context_menu.unwrap(), [MenuItem::Paste, MenuItem::Undo]);
}

// ----- opening the setup window

#[test]
fn opening_removes_sizeless_shapes_and_brings_strays_home() {
    let mut d = PreviewData::new();
    let keep = d.add_shape(hline(100, 100, 50));
    let sizeless = d.add_shape(Shape::new_line(p(5, 5), p(5, 5), 3, None, 1.0));
    let stray = d.add_shape(hline(-500, -500, 50));
    let far = d.add_shape(hline(9000, 100, 50));
    let mut s = DesignSession::new(d, None);
    s.set_background_size(800, 600);
    let (_, report) = s.setup_open();
    assert_eq!(report.removed, [sizeless]);
    assert_eq!(report.moved, [stray]);
    assert!(report.message.unwrap().contains("1 Preview Props"));
    assert_eq!(left_top(&s, stray), (10, 10));
    assert_eq!(left_top(&s, far), (5, 5), "past 8K is a rogue value, and a blank background resets it");
    assert_eq!(left_top(&s, keep), (100, 100));
    assert!(s.background_size().is_some());
    assert_scene_fresh(&s);
}

#[test]
fn opening_drops_links_to_elements_that_are_gone() {
    let (tree, group) = tree_with_leaves();
    let leaf = tree.node(group).children[0];
    let mut d = PreviewData::new();
    let mut line = hline(100, 100, 100);
    vixen_preview::link::link_elements(&mut line, &[leaf], &tree);
    let a = d.add_shape(line);
    let empty = Arc::new(ElementTree::new());
    let mut s = DesignSession::new(d, Some(empty));
    s.setup_open();
    assert!(s.item(a).unwrap().shape.pixels_vec().iter().all(|q| q.node_id.is_nil()));
    assert_eq!(s.scene().node_of_light(0), None);
}

#[test]
fn checks_on_closing() {
    let mut t = ElementTree::new();
    let group = t.add_node(Uuid::new_v4(), "Group with no element", None);
    let group_id = t.node(group).id;
    let t = Arc::new(t);
    let mut d = PreviewData::new();
    let mut line = hline(100, 100, 100);
    line.set_string_type(StringType::Pixel);
    let a = d.add_shape(line);
    d.add_shape(hline(100, 150, 100));
    let mut s = DesignSession::new(d, Some(t));
    assert!(s.invalid_element_links().is_empty());
    // Link a pixel to the group, which has no element. Only an old or broken file can do this.
    s.edit_shape(a, |sh| {
        let mut e = sh.to_xml();
        let pixels = e.child_mut("Pixels").unwrap();
        let first = pixels.elements_mut().next().unwrap();
        first.push(XElement::with_text("http://schemas.datacontract.org/2004/07/VixenModules.Preview.VixenPreview.Shapes", "NodeId", group_id.to_string()));
        *sh = Shape::from_xml(&e);
    });
    assert_eq!(s.invalid_element_links(), [a]);
    assert!(!s.fixtures_without_opengl());
    let mut d = PreviewData::new();
    d.add_shape(Shape::new_moving_head(p(0, 0), None, 1.0));
    assert!(DesignSession::new(d, None).fixtures_without_opengl());
}

// ----- saving what was edited

#[test]
fn edits_survive_a_save_and_load() {
    let (mut s, _, b, _) = three_lines();
    click(&mut s, 150, 150, NONE);
    drag(&mut s, (150, 150), (170, 180), NONE);
    click(&mut s, 500, 500, NONE);
    s.add_shape(Tool::Rectangle);
    let lights = s.scene().lights.clone();
    let e = s.into_data().to_xml();
    let back = PreviewData::from_xml(&e).unwrap();
    let scene = vixen_preview::scene::Scene::build(&back, None);
    assert_eq!(scene.lights, lights);
    assert_eq!(back.display_items.len(), 4);
    let _ = b;
    let reparsed = XElement::parse(&e.to_document()).unwrap();
    assert_eq!(e.first_difference(&reparsed, String::new()), None);
}

#[test]
fn hostile_mouse_input_does_not_panic() {
    for extreme in [i32::MAX, i32::MIN] {
        let (mut s, a, b, _) = three_lines();
        s.set_scroll(extreme, extreme);
        s.mouse_move(extreme, extreme, NONE);
        s.mouse_down(extreme, extreme, NONE);
        s.mouse_move(extreme, extreme, NONE);
        s.mouse_up(extreme, extreme, NONE);
        s.set_scroll(0, 0);
        s.set_zoom(4.0);
        s.select_items(&[a, b]);
        s.mouse_down(150, 100, SHIFT);
        s.mouse_move(extreme, extreme.wrapping_neg(), NONE);
        s.mouse_up(extreme, 0, NONE);
        s.align_left();
        s.distribute_vertical();
        s.key_down(Key::Up, NONE);
        s.copy();
        s.paste();
        s.undo();
        s.undo();
        s.redo();
        let _ = s.overlay();
    }
}

// ----- what a window host needs

fn two_selected() -> (DesignSession, ShapeId, ShapeId) {
    let (mut s, a, b, _) = three_lines();
    click(&mut s, 110, 100, NONE);
    click(&mut s, 110, 150, CTRL);
    (s, a, b)
}

#[test]
fn undo_many_is_one_change_and_out_of_range_does_nothing() {
    let (mut s, _, _, _) = three_lines();
    let before = s.scene().lights.clone();
    for _ in 0..3 {
        s.select_all();
        s.align_left();
    }
    assert_eq!(s.num_undoable(), 3);
    let nothing = s.undo_many(4);
    assert!(!nothing.undo_changed);
    assert_eq!(s.num_undoable(), 3);
    let ch = s.undo_many(3);
    assert!(ch.undo_changed);
    assert_eq!((s.num_undoable(), s.num_redoable()), (0, 3));
    assert_eq!(s.scene().lights, before);
    s.redo_many(2);
    assert_eq!((s.num_undoable(), s.num_redoable()), (2, 1));
}

#[test]
fn toolbar_state_follows_the_selection_rules() {
    let (mut s, a, _b) = two_selected();
    let t = s.toolbar_state();
    assert!(t.align && t.bulb && t.lock && !t.unlock && !t.unlock_all);
    s.lock_selected();
    let t = s.toolbar_state();
    assert!(!t.align && !t.bulb && !t.lock && !t.unlock && t.unlock_all && t.hide_locked_enabled);
    assert!(t.can_undo && !t.can_redo);
    s.undo();
    assert!(s.toolbar_state().can_redo);
    // A single locked item decides alone: Shift is how a locked prop is picked.
    click(&mut s, 110, 100, NONE);
    s.lock_selected();
    click(&mut s, 110, 100, SHIFT);
    assert_eq!(s.selected_item(), Some(a));
    let t = s.toolbar_state();
    assert!(!t.lock && t.unlock);
    assert_eq!(t.tool, Tool::Select);
}

#[test]
fn unlock_all_without_undo_pushes_nothing() {
    let (mut s, _a, _b) = two_selected();
    s.lock_selected();
    let n = s.num_undoable();
    let ch = s.unlock_all_without_undo();
    assert!(ch.light_colors_changed);
    assert_eq!(s.num_undoable(), n);
    assert!(!s.toolbar_state().unlock_all);
}

#[test]
fn a_blank_background_takes_the_view_size_then_replaces_both_sides_if_a_shape_is_past_either() {
    let mut d = PreviewData::new();
    d.add_shape(Shape::new_line(p(1000, 50), p(1200, 100), 11, None, 1.0));
    let mut s = DesignSession::new(d, None);
    s.view_resized(1000, 600);
    assert_eq!(s.background_size(), Some((1201, 101)));
    // An image keeps its size.
    s.set_background_file(Some("a.png".to_string()));
    s.set_background_size(640, 480);
    s.view_resized(300, 200);
    assert_eq!(s.background_size(), Some((640, 480)));
    assert_eq!(s.scroll_max(), (340, 280));
}

#[test]
fn naming_a_group_keeps_upstreams_order_of_group_then_ask() {
    let (mut s, _a, _b) = two_selected();
    let ch = s.group_selected();
    assert!(ch.message.is_none());
    let group = s.selected_item().unwrap();
    let (_, xml) = s.name_group_as_template(group, "Tree 1");
    let xml = xml.unwrap();
    assert!(xml.contains("Tree 1"), "{xml}");
    assert!(xml.contains("DisplayItem"));
}

#[test]
fn a_groups_strings_are_listed_and_edited_one_by_one() {
    let (mut s, _a, _b) = two_selected();
    s.group_selected();
    let group = s.selected_item().unwrap();
    assert_eq!(s.string_names(group).len(), 2);
    let rows = s.string_properties(group, 1);
    assert!(rows.iter().any(|r| r.id == "PixelSize"));
    let ch = s.set_string_property(group, 1, "PixelSize", PropertyValue::Int(9)).unwrap();
    assert!(!ch.scene.is_empty());
    s.sync_string_light_sizes(group, 1);
    assert!(s.string_properties(group, 0).iter().any(|r| r.id == "PixelSize" && r.value == PropertyValue::Int(9)));
    assert!(s.set_string_property(group, 7, "PixelSize", PropertyValue::Int(9)).is_err());
}

#[test]
fn property_rows_follow_the_grid_not_the_class() {
    let arch = Shape::new_arch(p(10, 10), None, 1.0);
    let names: Vec<_> = arch.properties_for_grid(false).iter().map(|r| (r.category, r.name)).collect();
    assert_eq!(
        names,
        [
            ("Control", "Locked"),
            ("Location", "Common Location"),
            ("Position", "Bottom Right"),
            ("Position", "Rotation Angle"),
            ("Position", "Top Left"),
            ("Settings", "Light Count"),
            ("Settings", "Light Size"),
            ("Settings", "Linked Elements"),
            ("Settings", "Name"),
            ("Settings", "String Type"),
            ("Size", "Height"),
            ("Size", "Width"),
        ]
    );
    let names: Vec<_> = arch.properties_for_grid(true).iter().map(|r| r.name).collect();
    assert_eq!(
        names,
        ["Bottom Right", "Common Location", "Height", "Light Count", "Light Size", "Linked Elements", "Locked", "Name", "Rotation Angle", "String Type", "Top Left", "Width"]
    );
}

#[test]
fn template_names_and_the_all_black_check() {
    use vixen_preview::tools::is_all_black;
    use vixen_preview::valid_template_name;
    assert!(!valid_template_name("a:b"));
    assert!(!valid_template_name("a/b"));
    assert!(!valid_template_name("a?b"));
    assert!(valid_template_name(""));
    assert!(valid_template_name("Tree 1"));
    assert!(is_all_black(&[0, 0, 0, 255, 0, 0, 0, 255]));
    assert!(!is_all_black(&[0, 0, 0, 255, 1, 0, 0, 255]));
    assert!(!is_all_black(&[0, 0, 0, 0]));
}

#[test]
fn linker_opens_a_group_link_as_its_first_element_child_and_lists_rows_in_bulk() {
    let mut t = ElementTree::new();
    let group = t.add_node(Uuid::from_u128(1), "Group", None);
    let e1 = t.add_element("One");
    let e2 = t.add_element("Two");
    t.add_child(group, e1);
    t.add_child(group, e2);
    let empty = t.add_node(Uuid::from_u128(2), "Empty", None);
    let mut shape = Shape::new_line(p(0, 0), p(100, 0), 3, None, 1.0);
    shape.set_string_type(StringType::Pixel);
    let group_id = t.node(group).id;
    let empty_id = t.node(empty).id;
    let mut n = 0;
    shape.for_each_pixel_mut(&mut |px| {
        px.node_id = [group_id, empty_id, Uuid::nil()][n];
        n += 1;
    });
    let mut linker = ElementLinker::new(&shape);
    linker.resolve_group_links(&t);
    let rows = linker.rows(0, &t);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].element_name, "One");
    assert_eq!(rows[0].node, Some(t.node(e1).id));
    // A group with no element children stays.
    assert_eq!(rows[1].node, Some(empty_id));
    assert_eq!(rows[1].element_name, "Empty");
    assert_eq!((rows[2].number, rows[2].node, rows[2].element_name.as_str()), (3, None, ""));
}

#[test]
fn linked_pixel_count_counts_pixels_not_colours_and_only_those_with_elements() {
    let mut t = ElementTree::new();
    let element = t.add_element("One");
    t.node_mut(element).properties.push(PropertyInstance {
        type_id: property_ids::COLOR,
        instance_id: Uuid::new_v4(),
        data: PropertyData::Color { color_type: ElementColorType::SingleColor, single_color: Color::RED, color_set_name: None },
    });
    let id = t.node(element).id;
    let mut shape = Shape::new_line(p(0, 0), p(100, 0), 10, None, 1.0);
    let mut n = 0;
    shape.for_each_pixel_mut(&mut |px| {
        if n < 4 {
            px.node_id = id;
        }
        n += 1;
    });
    let mut d = PreviewData::new();
    d.add_shape(shape);
    let scene = vixen_preview::Scene::build(&d, Some(&t));
    assert_eq!(scene.linked_pixel_count(&t), 4);
}
