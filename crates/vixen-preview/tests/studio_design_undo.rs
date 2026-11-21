//! Studio: the designer's live dialogs (Linked Elements, Link Element, Location Offset, Resize Background) are steps on the
//! designer's undo stack.

use std::sync::Arc;
use vixen_core::ElementTree;
use vixen_preview::data::PreviewData;
use vixen_preview::link::ElementLinker;
use vixen_preview::point::PreviewPoint;
use vixen_preview::session::DesignSession;
use vixen_preview::shape::properties::PropertyValue;
use vixen_preview::shape::{Shape, StringType};

fn pixel_line() -> Shape {
    let mut line = Shape::new_line(PreviewPoint::new(100, 100), PreviewPoint::new(200, 100), 11, None, 1.0);
    line.set_string_type(StringType::Pixel);
    line
}

#[test]
fn live_linking_is_one_undo_step_and_redoes() {
    let mut tree = ElementTree::new();
    let group = tree.add_string("Str", 11);
    let tree = Arc::new(tree);
    let mut d = PreviewData::new();
    let a = d.add_shape(pixel_line());
    let mut s = DesignSession::new(d, Some(Arc::clone(&tree)));
    let before = s.item(a).unwrap().shape.clone();

    // The sheet's first change goes on the stack; the next joins it.
    let mut linker = ElementLinker::new(&before);
    linker.drop_nodes(0, 0, &[group], &tree);
    let mut shape = before.clone();
    linker.clone().apply(&mut shape);
    let ch = s.studio_replace_shape(a, shape, Some((before.clone(), "Link Elements")));
    assert!(ch.undo_changed && ch.scene[0].links_changed);
    linker.reverse_string(0);
    let mut shape = before.clone();
    linker.apply(&mut shape);
    s.studio_replace_shape(a, shape, None);
    let first = tree.node(tree.node(group).children[0]).id;
    let last = tree.node(tree.node(group).children[10]).id;
    assert_eq!(s.scene().node_of_light(0), Some(last));
    assert_eq!(s.undo_descriptions(), ["Link Elements"]);

    s.undo();
    assert_eq!(s.scene().node_of_light(0), None, "Undo takes the whole sheet's linking back");
    assert_eq!(s.num_undoable(), 0);
    s.redo();
    assert_eq!(s.scene().node_of_light(0), Some(last));
    assert_ne!(first, last);
}

#[test]
fn an_undoable_shape_edit_records_only_real_changes() {
    let mut d = PreviewData::new();
    let a = d.add_shape(pixel_line());
    let mut s = DesignSession::new(d, None);
    let rename = |s: &mut DesignSession, name: &str| {
        s.studio_edit_shape_undoable(a, "Link Element", |s| s.set_shape_property(a, "Name", PropertyValue::Text(name.into()))).unwrap()
    };
    assert!(rename(&mut s, "Roof").undo_changed);
    assert!(!rename(&mut s, "Roof").undo_changed, "the same value adds no step");
    assert_eq!(s.undo_descriptions(), ["Link Element"]);
    s.undo();
    assert_ne!(s.item(a).unwrap().shape.name(), Some("Roof"));
    s.redo();
    assert_eq!(s.item(a).unwrap().shape.name(), Some("Roof"));
}

#[test]
fn location_offset_and_background_resize_undo() {
    let mut d = PreviewData::new();
    let a = d.add_shape(pixel_line());
    d.background_file_name = Some("yard.jpg".into());
    let mut s = DesignSession::new(d, None);
    s.set_background_size(800, 600);

    assert!(s.studio_set_location_offset(30.0, -5.0).undo_changed);
    assert!(!s.studio_set_location_offset(30.0, -5.0).undo_changed);
    assert_eq!(s.undo_descriptions(), ["Location Offset"]);

    let right = |s: &DesignSession| s.scene().lights.iter().map(|l| l.x).fold(0.0f32, f32::max);
    let width = right(&s);
    s.studio_resize_background(Some("yard-1600.jpg".into()), 1600, 1200, true);
    assert_eq!(s.background_size(), Some((1600, 1200)));
    assert_eq!(s.data().background_file_name.as_deref(), Some("yard-1600.jpg"));
    assert_ne!(right(&s), width, "the shapes scaled with it");
    assert_eq!(s.undo_descriptions(), ["Resize Background", "Location Offset"]);

    s.undo();
    assert_eq!(s.background_size(), Some((800, 600)));
    assert_eq!(s.data().background_file_name.as_deref(), Some("yard.jpg"));
    assert_eq!(right(&s), width);
    s.undo();
    assert_eq!(s.data().location_offset.x.value, 0.0);
    s.redo();
    s.redo();
    assert_eq!((s.data().location_offset.x.value, s.data().location_offset.y.value), (30.0, -5.0));
    assert_eq!(s.data().background_file_name.as_deref(), Some("yard-1600.jpg"));
}

#[test]
fn props_read_their_kind_lights_and_links_and_tab_walks_them() {
    let mut tree = ElementTree::new();
    let group = tree.add_string("Roofline", 11);
    let tree = Arc::new(tree);
    let mut d = PreviewData::new();
    let a = d.add_shape(pixel_line());
    let b = d.add_shape(pixel_line());
    let mut s = DesignSession::new(d, Some(Arc::clone(&tree)));
    let before = s.item(a).unwrap().shape.clone();
    let mut linker = ElementLinker::new(&before);
    linker.drop_nodes(0, 0, &[group], &tree);
    let mut linked = before.clone();
    linker.apply(&mut linked);
    s.studio_replace_shape(a, linked, None);

    let items = s.studio_access_items();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].label, "Line, 11 lights, linked to Roofline");
    assert_eq!(items[1].label, "Line, 11 lights, not linked");
    assert!(items[0].w > 0 && items[0].h > 0);
    assert!(items.iter().all(|i| !i.selected));

    // Tab from nothing picks the first prop, then the next, then wraps; ⇧Tab goes back.
    let (ch, said) = s.studio_access_step(1);
    assert!(ch.selection_changed && ch.show_properties == Some(a));
    assert_eq!(said.as_deref(), Some("Line, 11 lights, linked to Roofline"));
    assert_eq!(s.selected_item(), Some(a));
    s.studio_access_step(1);
    assert_eq!(s.selected_item(), Some(b));
    s.studio_access_step(1);
    assert_eq!(s.selected_item(), Some(a), "wraps around");
    s.studio_access_step(-1);
    assert_eq!(s.selected_item(), Some(b));
    assert!(s.studio_access_items()[1].selected);
    assert_eq!(s.num_undoable(), 0, "selecting is not an edit");
}
