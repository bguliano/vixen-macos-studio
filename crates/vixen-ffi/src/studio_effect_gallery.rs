//! The sequencer's Effect Gallery sheet (#62): looping thumbnails of effect variants on the selected row's prop, from
//! the same code as the effect_gallery MCP tool. Touches no document; placing one is an ordinary effect_add.
use crate::core::FfiError;
use crate::sequence::SequencerSession;

#[derive(uniffi::Record)]
pub struct StudioGalleryVariant {
    pub label: String,
    pub type_id: String,
    /// PNG frames, evenly through the variant, to loop.
    pub frames: Vec<Vec<u8>>,
    /// The most lights one frame lit; 0 when the effect left the prop dark.
    pub illuminated: u32,
}

#[derive(uniffi::Record)]
pub struct StudioGallery {
    /// The preview whose geometry the thumbnails use.
    pub preview_name: String,
    pub variants: Vec<StudioGalleryVariant>,
}

#[uniffi::export]
impl SequencerSession {
    /// effect_gallery's frames: arguments as the tool takes them (targetId, variants, durationMs, previewId), `frames`
    /// PNGs per variant at most `tile_width` wide.
    pub fn studio_effect_gallery(
        &self,
        arguments_json: String,
        frames: u32,
        tile_width: u32,
    ) -> Result<StudioGallery, FfiError> {
        let args: serde_json::Value =
            serde_json::from_str(&arguments_json).map_err(|e| FfiError::new(e.to_string()))?;
        let (variants, preview_name) = vixen_session::studio_sequence::effect_gallery::thumbnails(
            &self.core,
            &args,
            frames.clamp(1, 24) as usize,
            tile_width.clamp(48, 320),
        )
        .map_err(FfiError::new)?;
        Ok(StudioGallery {
            preview_name,
            variants: variants
                .into_iter()
                .map(|v| StudioGalleryVariant {
                    label: v.label,
                    type_id: v.type_id.to_string(),
                    frames: v.frames,
                    illuminated: u32::try_from(v.illuminated).unwrap_or(u32::MAX),
                })
                .collect(),
        })
    }
}
