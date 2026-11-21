//! Previews: the list, live windows and the designer, all drawn by the GPU renderer into a Swift-owned `CAMetalLayer`.
//!
//! A mouse event in the designer is one call here, which changes the session and tells the renderer exactly what moved, and it comes
//! back with a small record that is empty unless the window has something to do. Light data never crosses to Swift.

use crate::core::{FfiError, VixenCore};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;
use vixen_core::ElementTree;
use vixen_session::app::AppCore;
use vixen_session::instrumentation::MillisecondsValue;
use vixen_session::preview::{background_path, load_background, load_preview_data, preview_entries, LiveCounters, LiveTarget, PreviewEntry, MODULE_FOLDER};
use vixen_session::preview_design::{background_dimensions, import_background, load_background_checked, resize_background_image, save_preview_data, BackgroundLoad, DesignerHost, DesignerOutcome};
use vixen_session::preview_live::{load_live_window_state, restore_bounds, LiveView, LiveWindowState};
use vixen_session::render_support::{design, LiveRenderer, Scene};

// STUDIO-BEGIN designer-live-dialogs: the designer's Linked Elements, Link Element, Location Offset and Resize Background are live, with Undo
#[path = "studio_preview_designer.rs"]
mod studio_preview_designer;
// STUDIO-END designer-live-dialogs

/// A preview in the system configuration.
#[derive(uniffi::Record)]
pub struct PreviewRow {
    pub id: String,
    pub name: String,
}

/// A live preview's renderer, as measured.
#[derive(uniffi::Record)]
pub struct LivePreviewStats {
    pub frames_rendered: u64,
    pub frames_dropped: u64,
    pub last_frame_cpu_ms: f64,
    pub last_error: Option<String>,
    /// The status bar's `Lights`: pixels linked to something with an element.
    pub light_count: u32,
    pub frames_occluded: u64,
    pub frames_timed_out: u64,
    /// Every light the renderer draws: one per colour of a discrete element, linked or not.
    pub scene_lights: u32,
    /// Published colour frames the render thread took.
    pub colors_taken: u64,
    /// Engine frames that reached this window, and those in which one of its colours changed. Only those go to the renderer.
    pub engine_frames: u64,
    pub changed_frames: u64,
}

/// What the status bar shows.
#[derive(uniffi::Record)]
pub struct LivePreviewStatus {
    pub lights: u32,
    pub fps: u32,
    /// `Distance`, in the camera view: the camera's z.
    pub distance: f32,
}

#[derive(uniffi::Record)]
pub struct LiveFixtureLegend {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub font_size: f32,
}

/// A rectangle in points, top left and y down, as the settings keep window positions.
#[derive(uniffi::Record, Copy, Clone)]
pub struct PreviewRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Where a live window opens.
#[derive(uniffi::Record)]
pub struct LiveWindowBounds {
    /// None for the system's default placement.
    pub location: Option<PreviewPoint2>,
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
}

#[derive(uniffi::Record, Copy, Clone)]
pub struct PreviewPoint2 {
    pub x: i32,
    pub y: i32,
}

/// The toggles of the live window's context menu, with the window's own state.
#[derive(uniffi::Record, Clone)]
pub struct LiveWindowFlags {
    pub show_status: bool,
    pub always_on_top: bool,
    pub on_top_when_active: bool,
    pub transparent: bool,
    pub show_borders: bool,
    pub lock_position: bool,
    pub enable_light_scaling: bool,
    pub zoom_level: f64,
    /// "Normal", "Maximized" or "Minimized".
    pub window_state: String,
}

impl From<&LiveWindowState> for LiveWindowFlags {
    fn from(s: &LiveWindowState) -> LiveWindowFlags {
        LiveWindowFlags {
            show_status: s.show_status,
            always_on_top: s.always_on_top,
            on_top_when_active: s.on_top_when_active,
            transparent: s.transparent,
            show_borders: s.show_borders,
            lock_position: s.lock_position,
            enable_light_scaling: s.enable_light_scaling,
            zoom_level: s.zoom_level,
            window_state: s.window_state.clone(),
        }
    }
}

/// `Reset Size`'s answer: the content size to give the window.
#[derive(uniffi::Record, Copy, Clone)]
pub struct LiveSize {
    pub width: f64,
    pub height: f64,
}

/// A preview read and ready to show. Everything slow is done by the time this exists: the XML, the scene and the image. So it can
/// be made off the main thread, and opening the window after it doesn't stall anything.
#[derive(uniffi::Object)]
pub struct LivePreparation {
    core: Arc<AppCore>,
    id: Uuid,
    name: String,
    camera: bool,
    state: LiveWindowState,
    scene: Scene,
    background_image: Option<(Vec<u8>, u32, u32)>,
    background_alpha: u8,
    extent: (i32, i32),
    linked: u32,
    tree: Arc<ElementTree>,
    fixtures: vixen_session::preview_fixtures::FixturePreparation,
}

/// A live preview. Dropping it takes it off the frame stream and stops its renderer.
#[derive(uniffi::Object)]
pub struct LivePreview {
    core: Arc<AppCore>,
    id: Uuid,
    renderer: Arc<LiveRenderer>,
    target: u64,
    view: Mutex<LiveView>,
    linked: AtomicU32,
    scene_lights: AtomicU32,
    /// Frames rendered at the last status reading, and when.
    last_status: Mutex<(u64, Instant)>,
    /// `OpenGL preview update`, from the render thread's CPU time, in the camera view.
    update_value: Option<Arc<MillisecondsValue>>,
    /// `Update time for preview`, kept to carry over to a reload's target.
    update_time: Arc<MillisecondsValue>,
    counters: Arc<LiveCounters>,
    last_frames: AtomicU64,
}

const UPDATE_TIME_NAME: &str = "Update time for preview";
const OPENGL_UPDATE_NAME: &str = "OpenGL preview update";

/// A renderer on the window's layer. A UI test script that sets `VIXEN_PREVIEW_READ_PIXELS` gets drawables it can read back, to see what
/// was drawn. They are slower, so a script that measures leaves it unset.
///
/// # Safety
/// `metal_layer` is the address of a live `CAMetalLayer`.
unsafe fn renderer_for_layer(metal_layer: u64, width_px: u32, height_px: u32, scale: f32) -> Result<LiveRenderer, String> {
    let layer = metal_layer as usize as *mut std::ffi::c_void;
    let readable = std::env::var_os("VIXEN_UI_TEST").is_some() && std::env::var_os("VIXEN_PREVIEW_READ_PIXELS").is_some();
    // SAFETY: passed on from the caller.
    unsafe {
        if readable {
            LiveRenderer::for_layer_readable(layer, width_px, height_px, scale)
        } else {
            LiveRenderer::for_layer(layer, width_px, height_px, scale)
        }
        .map_err(|e| e.to_string())
    }
}

fn parse_id(id: &str) -> Result<Uuid, FfiError> {
    id.parse().map_err(|_| FfiError::new("That isn't a preview id."))
}

fn find_entry(core: &AppCore, id: &Uuid) -> Result<PreviewEntry, FfiError> {
    preview_entries(&core.system.read()).into_iter().find(|p| p.id == *id).ok_or_else(|| FfiError::new("That preview no longer exists."))
}

#[uniffi::export]
impl VixenCore {
    pub fn previews(&self) -> Vec<PreviewRow> {
        preview_entries(&self.inner.system.read()).into_iter().map(|p| PreviewRow { id: p.id.to_string(), name: p.name }).collect()
    }

    /// The previews that should be showing now, for the preview area to follow.
    pub fn running_preview_ids(&self) -> Vec<String> {
        self.inner.running_previews().iter().map(Uuid::to_string).collect()
    }

    /// Starts or stops a preview, as ticking it in Setup Previews does. For tests: this area otherwise only follows.
    pub fn live_preview_set_running(&self, id: String, running: bool) -> Result<bool, FfiError> {
        Ok(self.inner.set_preview_running(&parse_id(&id)?, running))
    }

    /// Calls `listener` after any preview starts or stops, on the thread that made the change.
    pub fn subscribe_preview_changes(&self, listener: Arc<dyn PreviewListener>) {
        self.inner.subscribe_previews(Arc::new(move || listener.changed()));
    }

    /// Calls `listener` when playback in a sequence editor starts or ends, for Auto On Top.
    pub fn subscribe_preview_player_active(&self, listener: Arc<dyn PreviewPlayerListener>) {
        self.inner.subscribe_player_active(Arc::new(move |active| listener.player_active(active)));
    }

    /// Reads a preview and everything a live window on it needs. Slow enough for a large show that it belongs off the main thread.
    pub fn prepare_live_preview(&self, id: String) -> Result<Arc<LivePreparation>, FfiError> {
        let id = parse_id(&id)?;
        let core = Arc::clone(&self.inner);
        let entry = find_entry(&core, &id)?;
        let system = core.system.read();
        let data = load_preview_data(&system.module_store, &entry)?;
        let tree = Arc::new(system.tree.clone());
        let fixtures = vixen_session::preview_fixtures::FixturePreparation::new(&data, &system, core.playback.elements());
        drop(system);
        let scene = Scene::build(&data, Some(&tree));
        let extent = data.display_items.iter().fold((0, 0), |(r, b), item| (r.max(item.shape.right()), b.max(item.shape.bottom())));
        let background_image = data.background_file_name.as_deref().filter(|n| !n.is_empty()).and_then(|name| load_background(&background_path(&core.paths, name)));
        let camera = data.use_open_gl;
        let state = load_live_window_state(&vixen_io::ProfileSettings::shared(), &id, camera);
        let linked = scene.linked_pixel_count(&tree);
        Ok(Arc::new(LivePreparation {
            id,
            name: entry.name,
            camera,
            state,
            background_alpha: data.background_alpha.clamp(0, 255) as u8,
            scene,
            background_image,
            extent,
            linked,
            tree,
            fixtures,
            core,
        }))
    }

    /// Opens the Preview Configuration window's session and renderer on a layer. See [`PreviewDesigner`].
    pub fn open_preview_designer(&self, id: String, metal_layer: u64, width_px: u32, height_px: u32, scale: f32, width_pt: i32, height_pt: i32) -> Result<Arc<PreviewDesigner>, FfiError> {
        let id = parse_id(&id)?;
        let core = Arc::clone(&self.inner);
        let entry = find_entry(&core, &id)?;
        let (data, tree) = {
            let system = core.system.read();
            (load_preview_data(&system.module_store, &entry)?, Arc::new(system.tree.clone()))
        };
        // SAFETY: the caller passes the address of a live CAMetalLayer, and wgpu retains it.
        let renderer = unsafe { renderer_for_layer(metal_layer, width_px, height_px, scale) }
            .map_err(|e| FfiError::new(format!("The designer couldn't start drawing: {e}")))?;
        let renderer = Arc::new(renderer);
        renderer.set_background_alpha(data.background_alpha.clamp(0, 255) as u8);

        let mut session = design::DesignSession::new(data, Some(Arc::clone(&tree)));
        session.set_view_size(width_pt, height_pt);
        // F2, in its order: the background's size first, because the repair's wrap uses it. Decoding comes after the window is up.
        let mut needs_decode = false;
        let file = session.data().background_file_name.clone().filter(|n| !n.is_empty());
        if let Some(name) = file {
            let path = background_path(&core.paths, &name);
            if !path.is_file() {
                // Q13: forgotten, by upstream's rule, with no message for a missing file.
                session.set_background_file(None);
            } else {
                if let Some((w, h)) = background_dimensions(&path) {
                    session.set_background_size(w as i32, h as i32);
                }
                needs_decode = true;
            }
        }
        if session.background_is_blank() || session.background_size().is_none() {
            session.set_background_size(width_pt, height_pt);
        }
        session.set_tool(design::Tool::Select);
        let (opened, report) = session.setup_open();
        let mut host = DesignerHost::new(session, renderer);
        host.apply(opened);
        let initial = host.apply_all();
        let verify_message = report.message.clone();
        Ok(Arc::new(PreviewDesigner {
            core,
            entry,
            host: Mutex::new(host),
            tree: Mutex::new(tree),
            initial: Mutex::new(Some(initial)),
            verify_message: Mutex::new(verify_message),
            needs_decode: AtomicU32::new(needs_decode as u32),
        }))
    }
}

/// The element templates the designer's Add list offers, in the list's order.
#[uniffi::export]
pub fn element_template_names() -> Vec<String> {
    vixen_session::setup::templates::ALL_TEMPLATES.iter().map(|(_, name)| name.to_string()).collect()
}

/// Told when a preview starts or stops.
#[uniffi::export(with_foreign)]
pub trait PreviewListener: Send + Sync {
    fn changed(&self);
}

/// Told when sequence playback starts or ends.
#[uniffi::export(with_foreign)]
pub trait PreviewPlayerListener: Send + Sync {
    fn player_active(&self, active: bool);
}

#[uniffi::export]
impl LivePreparation {
    pub fn name(&self) -> String {
        self.name.clone()
    }

    /// `UseOpenGL`: the camera view rather than the flat one.
    pub fn is_camera(&self) -> bool {
        self.camera
    }

    pub fn flags(&self) -> LiveWindowFlags {
        (&self.state).into()
    }

    /// `RestoreWindowState`'s decisions. `screens` are the working areas, and the sizes are in points: a frame size in the flat view,
    /// a content size in the camera view. `frame_extra_*` is what a frame adds to its content.
    pub fn bounds(&self, screens: Vec<PreviewRect>, frame_extra_width: i32, frame_extra_height: i32) -> LiveWindowBounds {
        let screens: Vec<_> = screens.iter().map(|s| design::Rect::new(s.x, s.y, s.width, s.height)).collect();
        let background = self.background_image.as_ref().map(|(_, w, h)| (*w, *h));
        let b = restore_bounds(&self.state, self.camera, background, self.extent, &screens, (frame_extra_width, frame_extra_height));
        LiveWindowBounds { location: b.location.map(|(x, y)| PreviewPoint2 { x, y }), width: b.width, height: b.height, maximized: b.maximized }
    }

    /// Starts drawing into a layer. `metal_layer` is the address of a live `CAMetalLayer`, and sizes are in physical pixels. Returns right away.
    pub fn open(&self, metal_layer: u64, width_px: u32, height_px: u32, scale: f32, client_width: f32, client_height: f32, frame_height: f32) -> Result<Arc<LivePreview>, FfiError> {
        let core = Arc::clone(&self.core);
        // SAFETY: the caller passes the address of a live CAMetalLayer, and wgpu retains it.
        let renderer = unsafe { renderer_for_layer(metal_layer, width_px, height_px, scale) }
            .map_err(|e| FfiError::new(format!("The preview couldn't start drawing: {e}")))?;
        let renderer = Arc::new(renderer);
        let background = self.background_image.as_ref().map(|(_, w, h)| (*w, *h));
        if let Some((pixels, w, h)) = &self.background_image {
            renderer.set_background(pixels, *w, *h);
        }
        renderer.set_background_alpha(self.background_alpha);
        let update_time = Arc::new(MillisecondsValue::new(UPDATE_TIME_NAME));
        core.instrumentation.add(update_time.clone());
        let counters = Arc::new(LiveCounters::default());
        let target = LiveTarget::new(Arc::clone(&renderer), &self.scene, &self.tree, core.playback.elements())
            .with_fixtures(self.fixtures.clone())
            .with_update_time(update_time.clone())
            .with_counters(counters.clone());
        let view = LiveView::new(self.camera, self.state.clone(), background, (client_width, client_height), frame_height, Arc::clone(&renderer));
        let target = core.playback.previews().add(target);
        let update_value = self.camera.then(|| {
            let value = Arc::new(MillisecondsValue::new(OPENGL_UPDATE_NAME));
            core.instrumentation.add(value.clone());
            value
        });
        Ok(Arc::new(LivePreview {
            scene_lights: AtomicU32::new(self.scene.lights.len() as u32),
            linked: AtomicU32::new(self.linked),
            core,
            id: self.id,
            renderer,
            target,
            view: Mutex::new(view),
            last_status: Mutex::new((0, Instant::now())),
            update_value,
            update_time,
            counters,
            last_frames: AtomicU64::new(0),
        }))
    }
}

#[uniffi::export]
impl LivePreview {
    /// The layer's size changed. Physical pixels, then the canvas and frame in points.
    pub fn resize(&self, width_px: u32, height_px: u32, scale: f32, client_width: f32, client_height: f32, frame_height: f32) {
        self.renderer.resize(width_px, height_px, scale);
        self.view.lock().set_client((client_width, client_height), frame_height);
    }

    pub fn stats(&self) -> LivePreviewStats {
        let stats = self.renderer.stats();
        LivePreviewStats {
            frames_rendered: stats.frames_rendered,
            frames_dropped: stats.frames_dropped,
            last_frame_cpu_ms: stats.last_frame_cpu.as_secs_f64() * 1000.0,
            last_error: stats.last_error,
            light_count: self.linked.load(Ordering::Relaxed),
            frames_occluded: stats.frames_occluded,
            frames_timed_out: stats.frames_timed_out,
            scene_lights: self.scene_lights.load(Ordering::Relaxed),
            colors_taken: stats.colors_taken,
            engine_frames: self.counters.engine_frames.load(Ordering::Relaxed),
            changed_frames: self.counters.changed_frames.load(Ordering::Relaxed),
        }
    }

    pub fn fixture_legends(&self) -> Vec<LiveFixtureLegend> {
        let legends = self.core.playback.previews().fixture_legends(self.target);
        let view = self.view.lock();
        legends.into_iter().filter_map(|legend| {
            let (x,y,scale) = view.project_fixture(legend.position)?;
            Some(LiveFixtureLegend { text: legend.text, x, y, font_size: 32.0 * scale })
        }).collect()
    }

    pub fn has_fixtures(&self) -> bool {
        self.core.playback.previews().has_fixtures(self.target)
    }

    /// The status bar's numbers. Read once a second: `fps` is the frames since the last reading.
    pub fn status(&self) -> LivePreviewStatus {
        let stats = self.renderer.stats();
        let mut last = self.last_status.lock();
        let elapsed = last.1.elapsed().as_secs_f64();
        let fps = if elapsed > 0.0 { (stats.frames_rendered.saturating_sub(last.0) as f64 / elapsed).round() as u32 } else { 0 };
        *last = (stats.frames_rendered, Instant::now());
        if let Some(value) = &self.update_value {
            if stats.frames_rendered != self.last_frames.swap(stats.frames_rendered, Ordering::Relaxed) {
                value.set(stats.last_frame_cpu.as_secs_f64() * 1000.0);
            }
        }
        LivePreviewStatus { lights: self.linked.load(Ordering::Relaxed), fps, distance: self.view.lock().camera.z }
    }

    /// The gaps between the last few hundred frames, oldest first, in milliseconds. For measuring pacing.
    pub fn frame_gaps_ms(&self) -> Vec<f64> {
        self.renderer.recent_frame_gaps().iter().map(|d| d.as_secs_f64() * 1000.0).collect()
    }

    /// The last frame presented, as RGBA rows: tests only, and never on the main thread. Empty until a visible window has drawn.
    pub fn read_pixels_for_test(&self) -> Vec<u8> {
        self.renderer.read_pixels()
    }

    /// Mouse wheel. Whole notches are 120.
    pub fn wheel(&self, delta: i32, shift: bool) {
        self.view.lock().wheel(delta, shift);
    }

    /// `I` and `O`.
    pub fn zoom_key(&self, zoom_in: bool, shift: bool) {
        self.view.lock().zoom_key(zoom_in, shift);
    }

    /// A left drag in the camera view: the pointer's previous position less its current one.
    pub fn drag(&self, dx: f32, dy: f32) {
        self.view.lock().drag(dx, dy);
    }

    pub fn reset_zoom(&self) {
        self.view.lock().reset_zoom();
    }

    /// The content size to give the window, or nothing in a camera view with no background.
    pub fn reset_size(&self) -> Option<LiveSize> {
        self.view.lock().reset_size().map(|(width, height)| LiveSize { width, height })
    }

    pub fn center(&self) {
        self.view.lock().center();
    }

    pub fn set_light_scaling(&self, on: bool) {
        self.view.lock().set_light_scaling(on);
    }

    pub fn set_transparent(&self, on: bool) {
        self.view.lock().set_transparent(on);
    }

    pub fn has_background(&self) -> bool {
        self.view.lock().background.is_some()
    }

    pub fn flags(&self) -> LiveWindowFlags {
        (&self.view.lock().state).into()
    }

    /// The menu's toggles. Doesn't save: call [`save_state`](Self::save_state).
    pub fn set_flags(&self, flags: LiveWindowFlags) {
        let mut view = self.view.lock();
        let s = &mut view.state;
        s.show_status = flags.show_status;
        s.always_on_top = flags.always_on_top;
        s.on_top_when_active = flags.on_top_when_active;
        s.show_borders = flags.show_borders;
        s.lock_position = flags.lock_position;
        s.window_state = flags.window_state;
    }

    /// Where the window is: top left, y down, a frame size in the flat view and a content size in the camera view.
    pub fn set_window_frame(&self, x: i32, y: i32, width: i32, height: i32) {
        let mut view = self.view.lock();
        view.state.location = Some((x, y));
        view.state.width = width;
        view.state.height = height;
    }

    /// Writes `Settings.xml`, with one write however many keys. Off the main thread if it can be: this is the disk.
    pub fn save_state(&self) {
        let view = self.view.lock();
        let (state, camera) = (view.state.clone(), view.camera_mode);
        drop(view);
        let id = self.id;
        std::thread::spawn(move || {
            if let Err(error) = vixen_session::preview_live::save_live_window_state(&vixen_io::ProfileSettings::shared(), &id, camera, &state) {
                vixen_session::logging::error("VixenPreview", "Couldn't save a preview's window state.", Some(&error.to_string()));
            }
        });
    }

    /// The preview's name, as it stands now. For following a rename.
    pub fn name(&self) -> String {
        self.core.system.read().previews.iter().find(|p| p.instance_id == self.id).map_or_else(String::new, |p| p.name.clone())
    }

    /// Reads the preview again and carries on in the same window, as `Setup()` does after its designer closes.
    pub fn reload(&self) -> Result<(), FfiError> {
        let entry = find_entry(&self.core, &self.id)?;
        let system = self.core.system.read();
        let data = load_preview_data(&system.module_store, &entry)?;
        let scene = Scene::build(&data, Some(&system.tree));
        self.linked.store(scene.linked_pixel_count(&system.tree), Ordering::Relaxed);
        self.scene_lights.store(scene.lights.len() as u32, Ordering::Relaxed);
        let target = LiveTarget::new(Arc::clone(&self.renderer), &scene, &system.tree, self.core.playback.elements())
            .with_fixtures(vixen_session::preview_fixtures::FixturePreparation::new(&data, &system, self.core.playback.elements()))
            .with_update_time(self.update_time.clone())
            .with_counters(self.counters.clone());
        self.core.playback.previews().replace(self.target, target);
        drop(system);
        let image = data.background_file_name.as_deref().filter(|n| !n.is_empty()).and_then(|name| load_background(&background_path(&self.core.paths, name)));
        match &image {
            Some((pixels, w, h)) => {
                self.renderer.set_background(pixels, *w, *h);
            }
            None => self.renderer.clear_background(),
        }
        self.renderer.set_background_alpha(data.background_alpha.clamp(0, 255) as u8);
        let mut view = self.view.lock();
        view.reload_background(image.as_ref().map(|(_, w, h)| (*w, *h)));
        Ok(())
    }
}

impl Drop for LivePreview {
    fn drop(&mut self) {
        let hub = self.core.playback.previews();
        hub.remove(self.target);
        if hub.is_empty() {
            self.core.instrumentation.remove(UPDATE_TIME_NAME);
            self.core.instrumentation.remove(OPENGL_UPDATE_NAME);
        }
    }
}

// ----- the designer

#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq, Debug)]
pub enum DesignerTool {
    Select,
    Light,
    Arch,
    Rectangle,
    Ellipse,
    Single,
    Triangle,
    MegaTree,
    Net,
    Flood,
    Star,
    Cane,
    PixelGrid,
    StarBurst,
    Icicle,
    PolyLine,
    MultiString,
    MovingHead,
}

impl From<design::Tool> for DesignerTool {
    fn from(t: design::Tool) -> DesignerTool {
        use design::Tool as T;
        match t {
            T::Select => DesignerTool::Select,
            T::String => DesignerTool::Light,
            T::Arch => DesignerTool::Arch,
            T::Rectangle => DesignerTool::Rectangle,
            T::Ellipse => DesignerTool::Ellipse,
            T::Single => DesignerTool::Single,
            T::Triangle => DesignerTool::Triangle,
            T::MegaTree => DesignerTool::MegaTree,
            T::Net => DesignerTool::Net,
            T::Flood => DesignerTool::Flood,
            T::Star => DesignerTool::Star,
            T::Cane => DesignerTool::Cane,
            T::PixelGrid => DesignerTool::PixelGrid,
            T::StarBurst => DesignerTool::StarBurst,
            T::Icicle => DesignerTool::Icicle,
            T::PolyLine => DesignerTool::PolyLine,
            T::MultiString => DesignerTool::MultiString,
            T::MovingHead => DesignerTool::MovingHead,
        }
    }
}

impl From<DesignerTool> for design::Tool {
    fn from(t: DesignerTool) -> design::Tool {
        use design::Tool as T;
        match t {
            DesignerTool::Select => T::Select,
            DesignerTool::Light => T::String,
            DesignerTool::Arch => T::Arch,
            DesignerTool::Rectangle => T::Rectangle,
            DesignerTool::Ellipse => T::Ellipse,
            DesignerTool::Single => T::Single,
            DesignerTool::Triangle => T::Triangle,
            DesignerTool::MegaTree => T::MegaTree,
            DesignerTool::Net => T::Net,
            DesignerTool::Flood => T::Flood,
            DesignerTool::Star => T::Star,
            DesignerTool::Cane => T::Cane,
            DesignerTool::PixelGrid => T::PixelGrid,
            DesignerTool::StarBurst => T::StarBurst,
            DesignerTool::Icicle => T::Icicle,
            DesignerTool::PolyLine => T::PolyLine,
            DesignerTool::MultiString => T::MultiString,
            DesignerTool::MovingHead => T::MovingHead,
        }
    }
}

#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq, Debug)]
pub enum DesignerCursor {
    Default,
    Cross,
    SizeAll,
    Rotate,
    InsertMode,
}

#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq, Debug)]
pub enum DesignerButton {
    Left,
    Right,
    Middle,
}

/// The keys `OnKeyDown` handles. ⌘ is passed as `DesignerMods.command`, which the session reads as Ctrl.
#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq, Debug)]
pub enum DesignerKey {
    Delete,
    Up,
    Down,
    Left,
    Right,
    Escape,
    Plus,
    Minus,
}

#[derive(uniffi::Record, Copy, Clone, Default)]
pub struct DesignerMods {
    pub command: bool,
    pub shift: bool,
    pub option: bool,
}

impl From<DesignerMods> for design::Modifiers {
    fn from(m: DesignerMods) -> design::Modifiers {
        design::Modifiers { ctrl: m.command, shift: m.shift, alt: m.option }
    }
}

#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq, Debug)]
pub enum DesignerMenuItem {
    Ungroup,
    Group,
    CreateTemplate,
    Cut,
    Copy,
    Paste,
    Delete,
    Undo,
    Redo,
    Lock,
    Unlock,
    SetZ,
}

/// The Align group: the first selected shape is the reference.
#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq, Debug)]
pub enum DesignerAlign {
    Left,
    Right,
    Top,
    Bottom,
    Horizontally,
    Vertically,
    DistributeHorizontally,
    DistributeVertically,
    MatchProperties,
}

#[derive(uniffi::Record, Copy, Clone, PartialEq, Eq)]
pub struct DesignerToolbarState {
    pub bulb: bool,
    pub lock: bool,
    pub unlock: bool,
    pub unlock_all: bool,
    pub align: bool,
    pub hide_locked_enabled: bool,
    pub hide_locked_active: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub tool: DesignerTool,
}

impl From<design::ToolbarState> for DesignerToolbarState {
    fn from(t: design::ToolbarState) -> DesignerToolbarState {
        DesignerToolbarState {
            bulb: t.bulb,
            lock: t.lock,
            unlock: t.unlock,
            unlock_all: t.unlock_all,
            align: t.align,
            hide_locked_enabled: t.hide_locked_enabled,
            hide_locked_active: t.hide_locked_active,
            can_undo: t.can_undo,
            can_redo: t.can_redo,
            tool: t.tool.into(),
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct DesignerLabel {
    pub x: f32,
    pub y: f32,
    pub text: String,
}

#[derive(uniffi::Record, Copy, Clone)]
pub struct DesignerPoint {
    pub x: i32,
    pub y: i32,
}

#[derive(uniffi::Record, Clone)]
pub struct DesignerTemplateRequest {
    pub shape: u32,
    pub template_name: String,
}

/// What the window has to do after an event. All empty for a drag that only moved lights.
#[derive(uniffi::Record, Clone, Default)]
pub struct DesignerChange {
    pub cursor: Option<DesignerCursor>,
    pub show_properties: Option<u32>,
    pub clear_properties: bool,
    pub elements_deselected: bool,
    pub element_template: Option<DesignerTemplateRequest>,
    pub context_menu: Option<Vec<DesignerMenuItem>>,
    pub zoom: Option<f64>,
    pub scroll: Option<DesignerPoint>,
    pub content_size: Option<DesignerPoint>,
    pub tool: Option<DesignerTool>,
    pub clipboard: Option<String>,
    pub message: Option<String>,
    pub lock_hint: bool,
    pub undo_changed: bool,
    pub toolbar: Option<DesignerToolbarState>,
    pub labels: Option<Vec<DesignerLabel>>,
    pub refresh_properties: bool,
}

impl From<DesignerOutcome> for DesignerChange {
    fn from(o: DesignerOutcome) -> DesignerChange {
        DesignerChange {
            cursor: o.cursor.map(|c| match c {
                design::Cursor::Default => DesignerCursor::Default,
                design::Cursor::Cross => DesignerCursor::Cross,
                design::Cursor::SizeAll => DesignerCursor::SizeAll,
                design::Cursor::Rotate => DesignerCursor::Rotate,
                design::Cursor::InsertMode => DesignerCursor::InsertMode,
            }),
            show_properties: o.show_properties.map(|s| s.0),
            clear_properties: o.clear_properties,
            elements_deselected: o.elements_deselected,
            element_template: o.element_template.map(|(s, name)| DesignerTemplateRequest { shape: s.0, template_name: name.to_string() }),
            context_menu: o.context_menu.map(|items| {
                items
                    .into_iter()
                    .map(|i| match i {
                        design::MenuItem::Ungroup => DesignerMenuItem::Ungroup,
                        design::MenuItem::Group => DesignerMenuItem::Group,
                        design::MenuItem::CreateTemplate => DesignerMenuItem::CreateTemplate,
                        design::MenuItem::Cut => DesignerMenuItem::Cut,
                        design::MenuItem::Copy => DesignerMenuItem::Copy,
                        design::MenuItem::Paste => DesignerMenuItem::Paste,
                        design::MenuItem::Delete => DesignerMenuItem::Delete,
                        design::MenuItem::Undo => DesignerMenuItem::Undo,
                        design::MenuItem::Redo => DesignerMenuItem::Redo,
                        design::MenuItem::Lock => DesignerMenuItem::Lock,
                        design::MenuItem::Unlock => DesignerMenuItem::Unlock,
                        design::MenuItem::SetZ => DesignerMenuItem::SetZ,
                    })
                    .collect()
            }),
            zoom: o.zoom,
            scroll: o.scroll.map(|(x, y)| DesignerPoint { x, y }),
            content_size: o.content_size.map(|(x, y)| DesignerPoint { x, y }),
            tool: o.tool.map(Into::into),
            clipboard: o.clipboard,
            message: o.message,
            lock_hint: o.lock_hint,
            undo_changed: o.undo_changed,
            toolbar: o.toolbar.map(Into::into),
            labels: o.labels.map(|labels| labels.into_iter().map(|l| DesignerLabel { x: l.x, y: l.y, text: l.text }).collect()),
            refresh_properties: o.refresh_properties,
        }
    }
}

/// A row's editor.
#[derive(uniffi::Enum, Clone, PartialEq)]
pub enum DesignerPropertyKind {
    Int,
    Bool,
    Text,
    Double,
    Point,
    Enumeration { members: Vec<String> },
    LinkedElements,
    LinkedElement,
}

#[derive(uniffi::Enum, Clone, PartialEq)]
pub enum DesignerPropertyValue {
    Int { value: i32 },
    Bool { value: bool },
    Text { value: String },
    Double { value: f64 },
    Point { x: i32, y: i32 },
    Enumeration { value: String },
    LinkedElements { strings: u32 },
    LinkedElement { node: Option<String> },
}

impl From<DesignerPropertyValue> for design::PropertyValue {
    fn from(v: DesignerPropertyValue) -> design::PropertyValue {
        use design::PropertyValue as P;
        match v {
            DesignerPropertyValue::Int { value } => P::Int(value),
            DesignerPropertyValue::Bool { value } => P::Bool(value),
            DesignerPropertyValue::Text { value } => P::Text(value),
            DesignerPropertyValue::Double { value } => P::Double(value),
            DesignerPropertyValue::Point { x, y } => P::Point { x, y },
            DesignerPropertyValue::Enumeration { value } => P::Enum(value),
            DesignerPropertyValue::LinkedElements { strings } => P::LinkedElements { strings: strings as usize },
            DesignerPropertyValue::LinkedElement { node } => P::LinkedElement(node.and_then(|n| n.parse().ok())),
        }
    }
}

/// One row of the property grid.
#[derive(uniffi::Record, Clone, PartialEq)]
pub struct DesignerProperty {
    pub category: String,
    pub name: String,
    pub id: String,
    pub description: String,
    pub kind: DesignerPropertyKind,
    pub value: DesignerPropertyValue,
    pub read_only: bool,
}

fn property_row(p: design::ShapeProperty) -> DesignerProperty {
    use design::PropertyValue as P;
    DesignerProperty {
        category: p.category.to_string(),
        name: p.name.to_string(),
        id: p.id.to_string(),
        description: p.description.to_string(),
        kind: match p.kind {
            design::PropertyKind::Int => DesignerPropertyKind::Int,
            design::PropertyKind::Bool => DesignerPropertyKind::Bool,
            design::PropertyKind::Text => DesignerPropertyKind::Text,
            design::PropertyKind::Double => DesignerPropertyKind::Double,
            design::PropertyKind::Point => DesignerPropertyKind::Point,
            design::PropertyKind::Enum(members) => DesignerPropertyKind::Enumeration { members: members.iter().map(|m| m.to_string()).collect() },
            design::PropertyKind::LinkedElements => DesignerPropertyKind::LinkedElements,
            design::PropertyKind::LinkedElement => DesignerPropertyKind::LinkedElement,
        },
        value: match p.value {
            P::Int(value) => DesignerPropertyValue::Int { value },
            P::Bool(value) => DesignerPropertyValue::Bool { value },
            P::Text(value) => DesignerPropertyValue::Text { value },
            P::Double(value) => DesignerPropertyValue::Double { value },
            P::Point { x, y } => DesignerPropertyValue::Point { x, y },
            P::Enum(value) => DesignerPropertyValue::Enumeration { value },
            P::LinkedElements { strings } => DesignerPropertyValue::LinkedElements { strings: strings as u32 },
            P::LinkedElement(node) => DesignerPropertyValue::LinkedElement { node: node.map(|n| n.to_string()) },
        },
        read_only: p.read_only,
    }
}

/// The panel's header for a shape: `setupControl.Title` and where its help button goes.
#[derive(uniffi::Record, Clone)]
pub struct DesignerShapeInfo {
    pub title: String,
    pub help_url: String,
    /// `PreviewCustom`: the panel with the string picker.
    pub is_group: bool,
    pub is_fixture: bool,
}

/// A row of the element tree.
#[derive(uniffi::Record, Clone)]
pub struct DesignerElementRow {
    pub id: String,
    pub name: String,
    pub has_children: bool,
    /// A node with an element, as opposed to a group.
    pub is_element: bool,
    pub tag_colors: Vec<u32>,
}

/// A row of the Linked Elements list.
#[derive(uniffi::Record, Clone)]
pub struct DesignerLinkRow {
    pub number: u32,
    pub node: Option<String>,
    pub element_name: String,
}

/// A template file, for Templates (Legacy).
#[derive(uniffi::Record, Clone)]
pub struct DesignerTemplateRow {
    pub name: String,
    pub file: String,
}

#[derive(uniffi::Record, Clone)]
pub struct DesignerFixtureNode {
    pub id: String,
    pub name: String,
}

/// The preview's settings, for the menu's check marks.
#[derive(uniffi::Record, Clone)]
pub struct DesignerSettings {
    pub save_locations: bool,
    pub keep_insert_mode: bool,
    pub use_open_gl: bool,
    pub location_offset_x: f64,
    pub location_offset_y: f64,
    pub background_alpha: i32,
    pub background_width: i32,
    pub background_height: i32,
    pub background_is_blank: bool,
    pub setup_left: i32,
    pub setup_top: i32,
    pub setup_width: i32,
    pub setup_height: i32,
    pub light_count: u32,
    pub shape_count: u32,
}

/// The Preview Configuration window: a design session with its renderer. Everything is applied to the renderer here.
#[derive(uniffi::Object)]
pub struct PreviewDesigner {
    core: Arc<AppCore>,
    entry: PreviewEntry,
    host: Mutex<DesignerHost>,
    tree: Mutex<Arc<ElementTree>>,
    initial: Mutex<Option<DesignerOutcome>>,
    verify_message: Mutex<Option<String>>,
    needs_decode: AtomicU32,
}

impl PreviewDesigner {
    fn run(&self, f: impl FnOnce(&mut design::DesignSession) -> design::Change) -> DesignerChange {
        let mut host = self.host.lock();
        let change = f(&mut host.session);
        host.apply(change).into()
    }

    fn shape_id(shape: u32) -> design::ShapeId {
        design::ShapeId(shape)
    }

    fn refreshed_tree(&self) -> Arc<ElementTree> {
        let tree = Arc::new(self.core.system.read().tree.clone());
        *self.tree.lock() = Arc::clone(&tree);
        tree
    }
}

#[uniffi::export]
impl PreviewDesigner {
    // The window's opening.

    /// What the first frame showed: the toolbar and document size, once.
    pub fn take_initial_change(&self) -> DesignerChange {
        self.initial.lock().take().map(Into::into).unwrap_or_default()
    }

    /// "Invalid Prop Locations", once.
    pub fn take_verify_message(&self) -> Option<String> {
        self.verify_message.lock().take()
    }

    /// Decodes the background. Slow for a large photo, so call it off the main thread, after the window shows. Returns the alert's text if
    /// the image was unreadable. `LoadBackground`'s outcomes: the file name is forgotten for anything but an image (F55, F56, Q13).
    pub fn load_background(&self) -> Option<String> {
        if self.needs_decode.swap(0, Ordering::Relaxed) == 0 {
            return None;
        }
        let name = self.host.lock().session.data().background_file_name.clone()?;
        let result = load_background_checked(&background_path(&self.core.paths, &name));
        let mut host = self.host.lock();
        match result {
            BackgroundLoad::Image { rgba, width, height } => {
                host.renderer().set_background(&rgba, width, height);
                host.session.set_background_size(width as i32, height as i32);
                None
            }
            BackgroundLoad::None | BackgroundLoad::AllBlack => {
                host.session.set_background_file(None);
                None
            }
            BackgroundLoad::Failed(message) => {
                host.session.set_background_file(None);
                Some(format!("There was an error loading the background image: {message}"))
            }
        }
    }

    // Events.

    pub fn mouse_down(&self, button: DesignerButton, x: i32, y: i32, mods: DesignerMods) -> DesignerChange {
        let button = match button {
            DesignerButton::Left => design::MouseButton::Left,
            DesignerButton::Right => design::MouseButton::Right,
            DesignerButton::Middle => design::MouseButton::Middle,
        };
        self.run(|s| s.mouse_down_with(button, x, y, mods.into()))
    }

    pub fn mouse_move(&self, x: i32, y: i32, mods: DesignerMods) -> DesignerChange {
        self.run(|s| s.mouse_move(x, y, mods.into()))
    }

    pub fn mouse_up(&self, button: DesignerButton, x: i32, y: i32, mods: DesignerMods) -> DesignerChange {
        let button = match button {
            DesignerButton::Left => design::MouseButton::Left,
            DesignerButton::Right => design::MouseButton::Right,
            DesignerButton::Middle => design::MouseButton::Middle,
        };
        self.run(|s| s.mouse_up_with(button, x, y, mods.into()))
    }

    pub fn key_down(&self, key: DesignerKey, mods: DesignerMods) -> DesignerChange {
        let key = match key {
            DesignerKey::Delete => design::Key::Delete,
            DesignerKey::Up => design::Key::Up,
            DesignerKey::Down => design::Key::Down,
            DesignerKey::Left => design::Key::Left,
            DesignerKey::Right => design::Key::Right,
            DesignerKey::Escape => design::Key::Escape,
            DesignerKey::Plus => design::Key::Plus,
            DesignerKey::Minus => design::Key::Minus,
        };
        self.run(|s| s.key_down(key, mods.into()))
    }

    /// The zoom slider and ⌘+ or ⌘-: `ZoomLevel`, clamped to 25% to 400%.
    pub fn set_zoom_level(&self, zoom: f64) -> DesignerChange {
        self.run(|s| s.set_zoom(zoom))
    }

    /// A wheel event: 120 is one notch.
    pub fn scroll_wheel(&self, delta: i32, x: i32, y: i32) -> DesignerChange {
        self.run(|s| s.mouse_wheel(delta, x, y))
    }

    pub fn magnify(&self, magnification: f64, x: i32, y: i32) -> DesignerChange {
        self.host.lock().magnify(magnification, x, y).into()
    }

    /// The scroll view moved. Only the view uniform changes.
    pub fn set_scroll(&self, h: i32, v: i32) {
        self.host.lock().set_scroll(h, v);
    }

    /// During a live resize: the drawable and the numbers, no more.
    pub fn view_resizing(&self, width_px: u32, height_px: u32, scale: f32, width_pt: i32, height_pt: i32) {
        let mut host = self.host.lock();
        host.renderer().resize(width_px, height_px, scale);
        host.session.set_view_size(width_pt, height_pt);
        host.sync_view();
    }

    /// A resize finished: `VixenPreviewControl_Resize`, which can change a blank background's size.
    pub fn view_resized(&self, width_pt: i32, height_pt: i32) -> DesignerChange {
        self.run(|s| s.view_resized(width_pt, height_pt))
    }

    // Toolbar.

    pub fn set_tool(&self, tool: DesignerTool) -> DesignerChange {
        self.run(|s| s.set_tool(tool.into()))
    }

    pub fn set_single_item_defaults(&self, name: String, start_index: i32, bulb_size: i32) {
        self.host.lock().session.set_single_item_defaults(&name, start_index, bulb_size);
    }

    pub fn toolbar_state(&self) -> DesignerToolbarState {
        self.host.lock().session.toolbar_state().into()
    }

    pub fn align(&self, kind: DesignerAlign) -> DesignerChange {
        self.run(|s| match kind {
            DesignerAlign::Left => s.align_left(),
            DesignerAlign::Right => s.align_right(),
            DesignerAlign::Top => s.align_top(),
            DesignerAlign::Bottom => s.align_bottom(),
            DesignerAlign::Horizontally => s.align_horizontal(),
            DesignerAlign::Vertically => s.align_vertical(),
            DesignerAlign::DistributeHorizontally => s.distribute_horizontal(),
            DesignerAlign::DistributeVertically => s.distribute_vertical(),
            DesignerAlign::MatchProperties => s.match_properties(),
        })
    }

    pub fn increase_bulb(&self) -> DesignerChange {
        self.run(|s| s.increase_bulb_size())
    }

    pub fn decrease_bulb(&self) -> DesignerChange {
        self.run(|s| s.decrease_bulb_size())
    }

    pub fn undo_many(&self, steps: u32) -> DesignerChange {
        self.run(|s| s.undo_many(steps as usize))
    }

    pub fn redo_many(&self, steps: u32) -> DesignerChange {
        self.run(|s| s.redo_many(steps as usize))
    }

    /// Newest first.
    pub fn undo_labels(&self) -> Vec<String> {
        self.host.lock().session.undo_descriptions()
    }

    /// Newest first.
    pub fn redo_labels(&self) -> Vec<String> {
        self.host.lock().session.redo_descriptions()
    }

    pub fn lock(&self) -> DesignerChange {
        self.run(|s| s.lock_selected())
    }

    pub fn unlock(&self) -> DesignerChange {
        self.run(|s| s.unlock_selected(false))
    }

    pub fn unlock_all(&self) -> DesignerChange {
        self.run(|s| s.unlock_all_without_undo())
    }

    pub fn set_hide_locked(&self, hide: bool) -> DesignerChange {
        self.run(|s| s.set_hide_locked(hide))
    }

    pub fn set_show_info(&self, show: bool) -> DesignerChange {
        self.run(|s| s.set_show_info(show))
    }

    pub fn select_all(&self) -> DesignerChange {
        self.run(|s| s.select_all())
    }

    pub fn select_shapes(&self, shapes: Vec<u32>) -> DesignerChange {
        let ids: Vec<_> = shapes.into_iter().map(Self::shape_id).collect();
        self.run(|s| s.select_items(&ids))
    }

    pub fn clear_selection(&self) -> DesignerChange {
        self.run(|s| s.clear_selection())
    }

    // Clipboard.

    pub fn set_clipboard_populated(&self, on: bool) {
        self.host.lock().session.set_clipboard_populated(on);
    }

    /// The text for the pasteboard comes back in `clipboard`.
    pub fn copy(&self) -> DesignerChange {
        let mut host = self.host.lock();
        let clipboard = host.session.copy();
        let mut change = design::Change::default();
        change.selection_changed = true;
        change.overlay_changed = true;
        change.light_colors_changed = true;
        change.clipboard = clipboard;
        host.apply(change).into()
    }

    pub fn cut(&self) -> DesignerChange {
        self.run(|s| s.cut())
    }

    pub fn paste_text(&self, text: String) -> DesignerChange {
        self.run(|s| s.paste_text(&text))
    }

    pub fn delete_selection(&self) -> DesignerChange {
        self.run(|s| s.delete_selected())
    }

    // Groups and templates.

    pub fn group(&self) -> DesignerChange {
        self.run(|s| s.group_selected())
    }

    pub fn ungroup(&self) -> DesignerChange {
        self.run(|s| s.ungroup_selected())
    }

    pub fn selected_shape(&self) -> Option<u32> {
        self.host.lock().session.selected_item().map(|s| s.0)
    }

    /// False for a name Windows couldn't hold. Templates stay portable.
    pub fn template_name_is_valid(&self, name: String) -> bool {
        design::valid_template_name(&name)
    }

    /// `CreateTemplate` after its dialog: names the group, selects it and writes the file.
    pub fn save_group_as_template(&self, shape: u32, name: String) -> Result<DesignerChange, FfiError> {
        let mut host = self.host.lock();
        let (change, xml) = host.session.name_group_as_template(Self::shape_id(shape), &name);
        let outcome: DesignerChange = host.apply(change).into();
        drop(host);
        if let Some(xml) = xml {
            self.template_store().save(&name, &xml).map_err(|e| FfiError::new(format!("The template couldn't be saved: {e}")))?;
        }
        Ok(outcome)
    }

    pub fn templates(&self) -> DesignerTemplates {
        let (entries, errors) = self.template_store().list();
        DesignerTemplates {
            rows: entries.into_iter().map(|e| DesignerTemplateRow { name: e.name.unwrap_or_default(), file: e.file.to_string_lossy().into_owned() }).collect(),
            errors,
        }
    }

    pub fn add_template_file(&self, file: String) -> Result<DesignerChange, FfiError> {
        let xml = self.template_store().read(std::path::Path::new(&file)).map_err(|e| FfiError::new(format!("The template couldn't be read: {e}")))?;
        Ok(self.run(|s| s.add_template(&xml)))
    }

    // Properties.

    pub fn shape_info(&self, shape: u32) -> Option<DesignerShapeInfo> {
        let host = self.host.lock();
        let item = host.session.item(Self::shape_id(shape))?;
        let s = &item.shape;
        const HOST: &str = "https://www.vixenlights.com/docs/usage/preview";
        let (is_group, is_fixture) = (matches!(s.kind(), design::ShapeKind::Custom { .. }), matches!(s.kind(), design::ShapeKind::MovingHead(_)));
        let help_url = match s.kind() {
            design::ShapeKind::Arch => format!("{HOST}/smart-objects/preview-arch/"),
            design::ShapeKind::Cane { .. } => format!("{HOST}/smart-objects/preview-candy-cane/"),
            design::ShapeKind::Icicle { .. } => format!("{HOST}/smart-objects/preview-icicles/"),
            design::ShapeKind::MegaTree { .. } => format!("{HOST}/smart-objects/preview-mega-tree/"),
            design::ShapeKind::Net { .. } => format!("{HOST}/smart-objects/preview-net/"),
            design::ShapeKind::Star { .. } => format!("{HOST}/smart-objects/preview-star/"),
            design::ShapeKind::Custom { .. } => "https://www.vixenlights.com/docs/".to_string(),
            _ => format!("{HOST}/preview-basic-shapes/"),
        };
        let title = if is_group { "Custom Prop Properties".to_string() } else { format!("{} Properties", s.type_name()) };
        Some(DesignerShapeInfo { title, help_url, is_group, is_fixture })
    }

    /// The rows in the grid's order.
    pub fn properties(&self, shape: u32, alphabetical: bool) -> Vec<DesignerProperty> {
        let host = self.host.lock();
        let Some(item) = host.session.item(Self::shape_id(shape)) else { return Vec::new() };
        item.shape.properties_for_grid(alphabetical).into_iter().map(property_row).collect()
    }

    /// An edit: `message` has the fixture's validation warning, if any.
    pub fn set_property(&self, shape: u32, id: String, value: DesignerPropertyValue) -> Result<DesignerChange, FfiError> {
        let mut host = self.host.lock();
        let change = host.session.set_shape_property(Self::shape_id(shape), &id, value.into()).map_err(|e| FfiError::new(e.to_string()))?;
        Ok(host.apply(change).into())
    }

    pub fn group_string_names(&self, shape: u32) -> Vec<String> {
        self.host.lock().session.string_names(Self::shape_id(shape))
    }

    pub fn group_string_properties(&self, shape: u32, string: u32, alphabetical: bool) -> Vec<DesignerProperty> {
        let mut rows = self.host.lock().session.string_properties(Self::shape_id(shape), string as usize);
        if alphabetical {
            rows.sort_by_cached_key(|r| r.name.to_lowercase());
        } else {
            rows.sort_by_cached_key(|r| (r.category.to_lowercase(), r.name.to_lowercase()));
        }
        rows.into_iter().map(property_row).collect()
    }

    pub fn set_group_string_property(&self, shape: u32, string: u32, id: String, value: DesignerPropertyValue) -> Result<DesignerChange, FfiError> {
        let mut host = self.host.lock();
        let change = host.session.set_string_property(Self::shape_id(shape), string as usize, &id, value.into()).map_err(|e| FfiError::new(e.to_string()))?;
        Ok(host.apply(change).into())
    }

    pub fn sync_string_light_sizes(&self, shape: u32, string: u32) -> DesignerChange {
        self.run(|s| s.sync_string_light_sizes(Self::shape_id(shape), string as usize))
    }

    pub fn fixture_nodes(&self) -> Vec<DesignerFixtureNode> {
        design::fixture_nodes(&self.tree.lock()).into_iter().map(|(id, name)| DesignerFixtureNode { id: id.to_string(), name }).collect()
    }

    // Elements.

    /// The children of a node, or the roots.
    pub fn element_rows(&self, parent: Option<String>) -> Vec<DesignerElementRow> {
        let tags = vixen_session::setup::tags::TagTable::new(&vixen_session::setup::tags::seeded_tags(&self.core.system.read().tags));
        let tree = self.tree.lock();
        let ids: Vec<_> = match parent.and_then(|p| p.parse::<Uuid>().ok()) {
            Some(id) => tree.find(&id).map_or_else(Vec::new, |n| tree.node(n).children.clone()),
            None => tree.roots.clone(),
        };
        ids.into_iter()
            .map(|n| {
                let node = tree.node(n);
                DesignerElementRow { id: node.id.to_string(), name: node.name.clone(), has_children: !node.children.is_empty(), is_element: node.element_id.is_some(), tag_colors: tags.colors_of(&node.tags) }
            })
            .collect()
    }

    /// The tree's selection, the row last clicked first: that one is what a new shape links to.
    pub fn set_selected_elements(&self, nodes: Vec<String>) -> DesignerChange {
        let ids: Vec<Uuid> = nodes.iter().filter_map(|n| n.parse().ok()).collect();
        self.run(|s| s.set_selected_nodes(&ids))
    }

    /// Each node's row in the tree: its ancestors' ids and its own joined by "/", along its first parent for a node in several groups.
    pub fn element_paths(&self, nodes: Vec<String>) -> Vec<String> {
        let tree = self.tree.lock();
        nodes
            .iter()
            .filter_map(|n| {
                let mut at = tree.find(&n.parse().ok()?)?;
                let mut parts = vec![tree.node(at).id.to_string()];
                while let Some(parent) = tree.node(at).parents.first().copied() {
                    parts.push(tree.node(parent).id.to_string());
                    at = parent;
                }
                parts.reverse();
                Some(parts.join("/"))
            })
            .collect()
    }

    /// The elements were edited outside the designer and applied to the system: the tree is read again, and links to nodes that are gone are dropped.
    pub fn reload_elements(&self) -> DesignerChange {
        let tree = self.refreshed_tree();
        self.run(|s| s.tree_replaced(tree))
    }

    /// After a template wizard asked for by [`DesignerChange`]'s `element_template`, or by the Elements panel's Add button with no `shape`: the tree
    /// with the new elements, the drawn shape linked to `nodes`, and `selection` selected in the tree.
    pub fn element_template_finished(&self, shape: Option<u32>, nodes: Vec<String>, selection: Vec<String>) -> DesignerChange {
        let tree = self.refreshed_tree();
        let nodes: Vec<Uuid> = nodes.iter().filter_map(|n| n.parse().ok()).collect();
        let selection: Vec<Uuid> = selection.iter().filter_map(|n| n.parse().ok()).collect();
        self.run(|s| s.element_template_finished(tree, shape.map(Self::shape_id), &nodes, &selection))
    }

    pub fn open_linker(self: Arc<Self>, shape: u32) -> Option<Arc<PreviewLinker>> {
        let tree = self.tree.lock().clone();
        let mut linker = {
            let host = self.host.lock();
            design::ElementLinker::new(&host.session.item(Self::shape_id(shape))?.shape)
        };
        linker.resolve_group_links(&tree);
        Some(Arc::new(PreviewLinker { designer: self, shape: Self::shape_id(shape), linker: Mutex::new(Some(linker)), tree, studio_live: studio_preview_designer::new_live_link() })) // STUDIO: designer-live-dialogs
    }

    /// Sets a moving head's one linked element, or clears it.
    pub fn set_fixture_node(&self, shape: u32, node: Option<String>) -> Result<DesignerChange, FfiError> {
        self.set_property(shape, "Node".to_string(), DesignerPropertyValue::LinkedElement { node })
    }

    // Settings and background.

    pub fn settings(&self) -> DesignerSettings {
        let host = self.host.lock();
        let session = &host.session;
        let d = session.data();
        let (bw, bh) = session.background_size().unwrap_or((0, 0));
        DesignerSettings {
            save_locations: d.save_locations,
            keep_insert_mode: d.keep_insert_mode,
            use_open_gl: d.use_open_gl,
            location_offset_x: d.location_offset.x.value,
            location_offset_y: d.location_offset.y.value,
            background_alpha: d.background_alpha,
            background_width: bw,
            background_height: bh,
            background_is_blank: session.background_is_blank(),
            setup_left: d.setup_left,
            setup_top: d.setup_top,
            setup_width: d.setup_width,
            setup_height: d.setup_height,
            light_count: session.scene().light_count() as u32,
            shape_count: d.display_items.len() as u32,
        }
    }

    pub fn set_save_locations(&self, on: bool) {
        self.host.lock().session.set_save_locations(on);
    }

    pub fn set_keep_insert_mode(&self, on: bool) {
        self.host.lock().session.set_keep_insert_mode(on);
    }

    pub fn set_use_open_gl(&self, on: bool) {
        self.host.lock().session.set_use_open_gl(on);
    }

    pub fn set_location_offset(&self, x: f64, y: f64) {
        self.host.lock().session.set_location_offset(x, y, 0.0);
    }

    pub fn set_setup_bounds(&self, left: i32, top: i32, width: i32, height: i32) {
        self.host.lock().session.set_setup_bounds(left, top, width, height);
    }

    /// F5: the saved window bounds, top left and y down, if `PreviewWindowBounds.IsRecoverable` says they can be used on these screens'
    /// working areas. The owner's Windows positions usually can't.
    pub fn restorable_setup_bounds(&self, screens: Vec<PreviewRect>) -> Option<PreviewRect> {
        let host = self.host.lock();
        let d = host.session.data();
        let saved = design::Rect { x: d.setup_left, y: d.setup_top, width: d.setup_width, height: d.setup_height };
        let areas: Vec<_> = screens.iter().map(|s| design::Rect { x: s.x, y: s.y, width: s.width, height: s.height }).collect();
        design::window_is_recoverable(saved, &areas).then_some(PreviewRect { x: saved.x, y: saved.y, width: saved.width, height: saved.height })
    }

    pub fn set_z(&self, position: i32) -> DesignerChange {
        // Q1, undecided: the pixels change, and the location write that would overwrite each element's Y isn't passed on.
        self.run(|s| s.set_z(position))
    }

    /// The slider. A uniform: nothing is rebuilt, however fast it's dragged.
    pub fn set_background_alpha(&self, alpha: i32) {
        let mut host = self.host.lock();
        host.session.set_background_alpha(alpha);
        host.renderer().set_background_alpha(alpha.clamp(0, 255) as u8);
    }

    /// Set Background: copies the file and shows it, with the slider at its maximum. Off the main thread: it decodes the image.
    pub fn choose_background(&self, path: String) -> Result<DesignerChange, FfiError> {
        let name = import_background(&self.core.paths, std::path::Path::new(&path))?;
        let result = load_background_checked(&background_path(&self.core.paths, &name));
        let mut host = self.host.lock();
        host.session.set_background_file(Some(name));
        match result {
            BackgroundLoad::Image { rgba, width, height } => {
                host.renderer().set_background(&rgba, width, height);
                host.session.set_background_size(width as i32, height as i32);
                host.session.set_background_alpha(255);
                host.renderer().set_background_alpha(255);
            }
            BackgroundLoad::None | BackgroundLoad::AllBlack => host.session.set_background_file(None),
            BackgroundLoad::Failed(message) => {
                host.session.set_background_file(None);
                return Err(FfiError::new(format!("There was an error loading the background image: {message}")));
            }
        }
        Ok(host.apply(design::Change::default()).into())
    }

    /// Background Properties, on OK: a new JPEG if there's an image, then the shapes if asked. Off the main thread: it resamples.
    pub fn resize_background(&self, width: i32, height: i32, scale_shapes: bool) -> Result<DesignerChange, FfiError> {
        let name = {
            let host = self.host.lock();
            host.session.data().background_file_name.clone().filter(|n| !n.is_empty())
        };
        if let Some(name) = name {
            let resized = resize_background_image(&self.core.paths, &name, width as u32, height as u32)?;
            let result = load_background_checked(&background_path(&self.core.paths, &resized));
            let mut host = self.host.lock();
            host.session.set_background_file(Some(resized));
            if let BackgroundLoad::Image { rgba, width, height } = result {
                host.renderer().set_background(&rgba, width, height);
            }
        }
        Ok(self.run(|s| s.resize_background(width, height, scale_shapes)))
    }

    // Close and save.

    /// F4's warning: shapes linked to a node with no element.
    pub fn invalid_link_shapes(&self) -> Vec<u32> {
        self.host.lock().session.invalid_element_links().into_iter().map(|s| s.0).collect()
    }

    /// F20: intelligent fixtures and the flat view.
    pub fn fixtures_without_open_gl(&self) -> bool {
        self.host.lock().session.fixtures_without_opengl()
    }

    /// `SaveLocationDataForElements`, then the preview into the module store. Nothing reaches disk. Off the main thread: a big show has
    /// many pixels. The session's lock is held only to copy.
    pub fn commit(&self) {
        let (data, updates) = {
            let host = self.host.lock();
            (host.session.data().clone(), host.session.location_updates())
        };
        // STUDIO-BEGIN preview-design-undo: the designer's write-back is one step on the show window's shared undo history
        let changed = vixen_session::studio_previews::design_changes(&self.core.system.read(), &self.entry, &data);
        self.core.studio_display.record_preview_edit(&self.core, "Edit Preview Layout", changed, |system| {
            design::apply_location_updates(&mut system.tree, &updates);
            save_preview_data(system, &self.entry, &data);
        });
        // STUDIO-END preview-design-undo
    }

    /// File > Save: [`commit`](Self::commit), then both config files. Stays open, with its undo intact.
    pub fn save(&self) -> Result<(), FfiError> {
        self.commit();
        self.core.save_system_config()?;
        self.core.save_module_config()?;
        Ok(())
    }

    /// Setup Previews must offer to keep or discard this. Whatever happened, as upstream.
    pub fn mark_changed(&self) {
        self.core.mark_preview_changed();
    }

    /// Stops the renderer without making the main thread wait for its thread. Call when the window closes.
    pub fn close(&self) {
        let renderer = Arc::clone(self.host.lock().renderer());
        std::thread::spawn(move || drop(renderer));
    }

    /// Draws everything now and reads it back: tests only, and never on the main thread.
    pub fn read_pixels_for_test(&self) -> Vec<u8> {
        let renderer = Arc::clone(self.host.lock().renderer());
        renderer.read_pixels()
    }

    pub fn stats(&self) -> LivePreviewStats {
        let host = self.host.lock();
        let stats = host.renderer().stats();
        LivePreviewStats {
            frames_rendered: stats.frames_rendered,
            frames_dropped: stats.frames_dropped,
            last_frame_cpu_ms: stats.last_frame_cpu.as_secs_f64() * 1000.0,
            last_error: stats.last_error,
            light_count: host.session.scene().light_count() as u32,
            frames_occluded: stats.frames_occluded,
            frames_timed_out: stats.frames_timed_out,
            scene_lights: host.session.scene().light_count() as u32,
            colors_taken: stats.colors_taken,
            engine_frames: 0,
            changed_frames: 0,
        }
    }

    pub fn frame_gaps_ms(&self) -> Vec<f64> {
        self.host.lock().renderer().recent_frame_gaps().iter().map(|d| d.as_secs_f64() * 1000.0).collect()
    }

    /// How many lights are linked to an element, for tests that edit the tree under them.
    pub fn linked_light_count_for_test(&self) -> u32 {
        let host = self.host.lock();
        let mut linked = 0;
        for item in &host.session.data().display_items {
            item.shape.for_each_pixel(&mut |p| linked += u32::from(!p.node_id.is_nil()));
        }
        linked
    }

    /// Where a light is in the view, for tests that need to press on one: shapes like a mega tree are only hit on their lights.
    pub fn light_view_point_for_test(&self, light: u32) -> Option<DesignerPoint> {
        let host = self.host.lock();
        let session = &host.session;
        let l = session.scene().lights.get(light as usize)?;
        let zoom = session.scene().zoom();
        let (h, v) = session.scroll();
        Some(DesignerPoint { x: (l.x as f64 * zoom) as i32 - h, y: (l.y as f64 * zoom) as i32 - v })
    }
}

impl PreviewDesigner {
    fn template_store(&self) -> design::TemplateStore {
        design::TemplateStore::new(&self.core.paths.module_data_dir().join(MODULE_FOLDER))
    }
}

/// Templates (Legacy)'s list, and the files that couldn't be read.
#[derive(uniffi::Record, Clone)]
pub struct DesignerTemplates {
    pub rows: Vec<DesignerTemplateRow>,
    pub errors: Vec<String>,
}

/// The Linked Elements dialog's state between opening and OK. Nothing reaches the shape until [`apply`](Self::apply).
#[derive(uniffi::Object)]
pub struct PreviewLinker {
    designer: Arc<PreviewDesigner>,
    shape: design::ShapeId,
    linker: Mutex<Option<design::ElementLinker>>,
    tree: Arc<ElementTree>,
    studio_live: Mutex<studio_preview_designer::LiveLink>, // STUDIO: designer-live-dialogs
}

impl PreviewLinker {
    fn with(&self, f: impl FnOnce(&mut design::ElementLinker)) {
        if let Some(linker) = self.linker.lock().as_mut() {
            f(linker);
        }
    }

    fn nodes(&self, ids: &[String]) -> Vec<vixen_core::NodeId> {
        ids.iter().filter_map(|id| id.parse::<Uuid>().ok()).filter_map(|id| self.tree.find(&id)).collect()
    }
}

#[uniffi::export]
impl PreviewLinker {
    /// The string combo's names.
    pub fn strings(&self) -> Vec<String> {
        self.linker.lock().as_ref().map_or_else(Vec::new, |l| l.strings().iter().take(l.visible_string_count()).map(|s| s.name.clone()).collect())
    }

    /// The list for a string, in bulk.
    pub fn rows(&self, string: u32) -> Vec<DesignerLinkRow> {
        self.linker.lock().as_ref().map_or_else(Vec::new, |l| {
            l.rows(string as usize, &self.tree).into_iter().map(|r| DesignerLinkRow { number: r.number, node: r.node.map(|n| n.to_string()), element_name: r.element_name }).collect()
        })
    }

    pub fn light_count_editable(&self) -> bool {
        self.linker.lock().as_ref().is_some_and(|l| l.light_count_editable())
    }

    pub fn light_count(&self, string: u32) -> u32 {
        self.linker.lock().as_ref().map_or(1, |l| l.light_count(string as usize) as u32)
    }

    /// A drop on a row: each node goes to that row and the ones after, by the dialog's rules.
    pub fn drop_nodes(&self, string: u32, row: u32, nodes: Vec<String>) {
        let nodes = self.nodes(&nodes);
        let tree = Arc::clone(&self.tree);
        self.with(|l| l.drop_nodes(string as usize, row as usize, &nodes, &tree));
    }

    /// A double-click in the tree: the node goes to each of these rows in turn.
    pub fn set_rows(&self, string: u32, rows: Vec<u32>, node: String) {
        let Some(node) = self.nodes(&[node]).first().copied() else { return };
        let tree = Arc::clone(&self.tree);
        self.with(|l| {
            for row in rows {
                l.set_linked(string as usize, row as usize, node, &tree);
            }
        });
    }

    pub fn copy_to_this_string(&self, string: u32, row: u32) {
        self.with(|l| l.copy_to_this_string(string as usize, row as usize));
    }

    pub fn copy_to_all_strings(&self, string: u32, row: u32) {
        self.with(|l| l.copy_to_all_strings(string as usize, row as usize));
    }

    pub fn reverse(&self, string: u32) {
        self.with(|l| l.reverse_string(string as usize));
    }

    pub fn set_light_count(&self, string: u32, count: u32) {
        self.with(|l| l.set_light_count(string as usize, count as usize));
    }

    /// OK.
    pub fn apply(&self) -> DesignerChange {
        let Some(linker) = self.linker.lock().take() else { return DesignerChange::default() };
        let shape = self.shape;
        self.designer.run(|s| s.edit_shape(shape, |shape| linker.apply(shape)))
    }
}

// Import Prop, F7 and F40.

/// A prop file read and waiting for the answers to its name tokens.
#[derive(uniffi::Object)]
pub struct PreviewPropFile {
    file: vixen_session::preview_import::PropFile,
}

/// `PhysicalMetadata.ColorMode`: which colour setup the import offers afterwards, and none for `Other`.
#[derive(uniffi::Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum PropColorMode {
    FullColor,
    Multiple,
    Single,
    Other,
}

#[uniffi::export]
impl PreviewPropFile {
    /// The `{1}` tokens in the prop's element names, each to be asked once, in this order.
    pub fn tokens(&self) -> Vec<String> {
        self.file.tokens()
    }

    pub fn color_mode(&self) -> PropColorMode {
        use vixen_session::preview_import::ColorMode;
        match self.file.color_mode {
            ColorMode::FullColor => PropColorMode::FullColor,
            ColorMode::Multiple => PropColorMode::Multiple,
            ColorMode::Single => PropColorMode::Single,
            ColorMode::Other => PropColorMode::Other,
        }
    }
}

#[derive(uniffi::Record)]
pub struct PropImportResult {
    pub change: DesignerChange,
    pub root: String,
    /// The new elements that hold lights: what the dimming curve and colour questions are about.
    pub light_nodes: Vec<String>,
}

#[uniffi::export]
impl PreviewDesigner {
    pub fn open_prop_file(&self, path: String) -> Result<Arc<PreviewPropFile>, FfiError> {
        Ok(Arc::new(PreviewPropFile { file: vixen_session::preview_import::PropFile::read(std::path::Path::new(&path))? }))
    }

    /// F44: imported face colours follow the discrete colour helper's selected colour before rebuilding the scene.
    pub fn refresh_imported_prop_colors(&self, root: String) -> DesignerChange {
        if let Ok(root) = Uuid::parse_str(&root) {
            vixen_session::preview_import::refresh_face_colors(&self.core, root);
        }
        self.reload_elements()
    }

    /// `AddPropToPreviewAsync`: the prop's elements join the running display, and its shape goes at `x`, `y`, which a drop gives in the
    /// canvas's own points and the scroll offset is added to.
    pub fn import_prop(&self, file: Arc<PreviewPropFile>, answers: std::collections::HashMap<String, String>, x: i32, y: i32, at_drop: bool) -> PropImportResult {
        let zoom = self.host.lock().session.zoom();
        let built = vixen_session::preview_import::build_live(&self.core, &file.file, zoom, &answers);
        let tree = self.refreshed_tree();
        let root = built.root;
        let shape = built.shape;
        let change = self.run(|s| {
            let (dx, dy) = if at_drop { s.scroll() } else { (0, 0) };
            s.custom_prop_imported(tree, root, shape, x + dx, y + dy)
        });
        PropImportResult { change, root: root.to_string(), light_nodes: built.light_nodes.iter().map(Uuid::to_string).collect() }
    }
}

