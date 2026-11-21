//! Studio: the live Image Maps sheet (Tools ▸ LipSync ▸ Edit Image Maps). Its edits are display commands (`image_map_*`, run with
//! `studio_display_command`), shared with agents and undone on the shared history; this adds the list, the pictures and the Image
//! Mapper's first choices.
use crate::core::VixenCore;
use crate::marks::image_maps::ImageMapperChoice;
use vixen_session::marks::map_library::{self as maps, MapperChoice};

#[uniffi::export]
impl VixenCore {
    /// The library as JSON: `{revision, phonemes, maps: [{name, notes, isDefault, editable, usingDefaults, pictures}]}`.
    pub fn studio_image_map_list(&self) -> String {
        vixen_session::studio_lipsync::list(&self.inner).to_string()
    }

    /// A map's picture for a phoneme (PNG, or BMP from Windows Vixen), or none for the "no image" tile.
    pub fn studio_image_map_picture(&self, name: String, phoneme: String) -> Option<Vec<u8>> {
        vixen_session::studio_lipsync::picture(&self.inner, &name, &phoneme)
    }
}

/// The Image Mapper's first choices for `file_count` dropped files, one per phoneme in the list's order (F102).
#[uniffi::export]
pub fn studio_image_map_mapper_choices(current: String, file_count: u32, mapped: Vec<String>) -> Vec<ImageMapperChoice> {
    vixen_session::studio_lipsync::mapper_choices(&current, file_count as usize, &mapped)
        .into_iter()
        .map(|c| match c {
            MapperChoice::Unmapped => ImageMapperChoice::Unmapped,
            MapperChoice::Mapped => ImageMapperChoice::Mapped,
            MapperChoice::File(i) => ImageMapperChoice::File { index: i as u32 },
        })
        .collect()
}

/// Whether a drop of these files is pictures the editor takes (F101).
#[uniffi::export]
pub fn studio_image_map_accepts_drop(files: Vec<String>) -> bool {
    maps::accepts_drop(&files.into_iter().map(std::path::PathBuf::from).collect::<Vec<_>>())
}
