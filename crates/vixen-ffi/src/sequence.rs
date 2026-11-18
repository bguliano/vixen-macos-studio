//! One open sequence, as every part of the sequencer window sees it. Areas add their own `#[uniffi::export] impl
//! SequencerSession` blocks in their own modules.

use crate::core::{FfiError, VixenCore};
use parking_lot::Mutex;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use vixen_core::TimeSpan;
use vixen_session::app::AppCore;
use vixen_session::sequence::SequenceSession;

/// A sequencer window's session.
#[derive(uniffi::Object)]
pub struct SequencerSession {
    pub core: Arc<AppCore>,
    pub session: Arc<Mutex<SequenceSession>>,
    playback_id: uuid::Uuid,
}

/// How rendering is going, for the status bar.
#[derive(uniffi::Record)]
pub struct RenderStatus {
    /// Effects still waiting for a render.
    pub waiting: u32,
    /// Bumps each time finished renders are applied, so a window knows when to redraw.
    pub revision: u64,
    pub failed: u32,
}

/// `SequenceExecutor`'s state, as the transport controls need it.
#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq)]
pub enum TransportState {
    Stopped,
    Playing,
    Paused,
}

impl SequencerSession {
    // STUDIO-BEGIN shared-sequence-wrapper: open agent-created documents without forking their undo/render state
    pub(crate) fn studio_wrap_shared(core: Arc<AppCore>, session: Arc<Mutex<SequenceSession>>) -> Arc<SequencerSession> {
        let playback_id=session.lock().playback_id();
        let finished=session.lock().take_finished_batches();
        if let Some(finished)=finished {
            let weak=Arc::downgrade(&session);
            std::thread::spawn(move || { while finished.recv().is_ok() { let Some(session)=weak.upgrade() else {break};session.lock().apply_finished_renders(); } });
        }
        Arc::new(SequencerSession {core,session,playback_id})
    }
    // STUDIO-END shared-sequence-wrapper

    pub(crate) fn wrap(core: Arc<AppCore>, mut session: SequenceSession) -> Arc<SequencerSession> {
        let playback_id = session.playback_id();
        let finished = session.take_finished_batches();
        let session = Arc::new(Mutex::new(session));
        core.register_sequence_session(&session);
        if let Some(finished) = finished {
            // Keeps finished renders flowing to the schedule even while the window is busy or hidden.
            let weak = Arc::downgrade(&session);
            let _ = std::thread::Builder::new().name("vixen-render-results".to_string()).spawn(move || {
                while finished.recv().is_ok() {
                    let Some(session) = weak.upgrade() else { break };
                    session.lock().apply_finished_renders();
                }
            });
        }
        Arc::new(SequencerSession { core, session, playback_id })
    }
}

#[uniffi::export]
impl VixenCore {
    /// Opens a sequence file. Audio decodes and effects render in the background.
    pub fn open_sequence(&self, path: String) -> Result<Arc<SequencerSession>, FfiError> {
        // STUDIO-BEGIN reuse-open-sequence: UI opens the same agent session and undo stack for an existing path
        let canonical=std::fs::canonicalize(&path).map_err(|e|FfiError::new(e.to_string()))?;
        if let Some(shared)=self.inner.open_sequence_sessions().into_iter().find(|s|s.lock().path().is_some_and(|p|std::fs::canonicalize(p).is_ok_and(|p|p==canonical))) {
            return Ok(SequencerSession::studio_wrap_shared(Arc::clone(&self.inner),shared));
        }
        // STUDIO-END reuse-open-sequence

        let env = self.inner.session_environment();
        let session = SequenceSession::open(Path::new(&path), &env)?;
        Ok(SequencerSession::wrap(Arc::clone(&self.inner), session))
    }

    /// A new, unsaved sequence.
    pub fn new_sequence(&self) -> Arc<SequencerSession> {
        let env = self.inner.session_environment();
        SequencerSession::wrap(Arc::clone(&self.inner), SequenceSession::new_untitled(&env))
    }
}

#[uniffi::export]
impl SequencerSession {
    /// The file name without its extension, or "Untitled".
    pub fn name(&self) -> String {
        self.session.lock().name()
    }

    pub fn file_path(&self) -> Option<String> {
        self.session.lock().path().map(|p| p.to_string_lossy().into_owned())
    }

    pub fn is_modified(&self) -> bool {
        self.session.lock().is_modified()
    }

    pub fn length_ms(&self) -> f64 {
        self.session.lock().length().total_ms()
    }

    pub fn effect_count(&self) -> u32 {
        self.session.lock().effect_count() as u32
    }

    /// Saves to `path`, or to where the sequence came from when that's empty.
    pub fn save(&self, path: Option<String>) -> Result<(), FfiError> {
        let mut session = self.session.lock();
        session.save(path.as_deref().map(Path::new))?;
        Ok(())
    }

    pub fn render_status(&self) -> RenderStatus {
        let session = self.session.lock();
        RenderStatus {
            waiting: session.unrendered_count() as u32,
            revision: session.counters.revision.load(Ordering::Relaxed),
            failed: session.counters.failed.load(Ordering::Relaxed) as u32,
        }
    }

    /// Messages about effects that failed to render and media that failed to load, since the last call.
    pub fn take_problems(&self) -> Vec<String> {
        self.session.lock().take_problems()
    }

    /// Starts rendering whatever has changed. Edits don't start it, so a drag can make many changes first.
    pub fn render_changes(&self) {
        self.session.lock().render_all_dirty();
    }

    // Transport

    /// `SequenceExecutor.Play`, clamped to the sequence's length.
    pub fn play(&self, start_ms: f64, end_ms: f64, looping: bool) {
        let playback = Arc::clone(&self.core.playback);
        let mut session = self.session.lock();
        playback.play(&mut session, TimeSpan::from_ms(start_ms), TimeSpan::from_ms(end_ms), looping);
    }

    pub fn pause(&self) {
        self.core.playback.pause_session(self.playback_id);
    }

    pub fn resume(&self) {
        self.core.playback.resume_session(self.playback_id);
    }

    pub fn stop(&self) {
        self.core.playback.stop_session(self.playback_id);
    }

    pub fn seek(&self, ms: f64) {
        self.core.playback.seek_session(self.playback_id, TimeSpan::from_ms(ms));
    }

    /// Reads this editor's position without taking the sequence/render lock.
    pub fn position_ms(&self) -> f64 {
        self.core.playback.session_position(self.playback_id).total_ms()
    }

    pub fn transport(&self) -> TransportState {
        match self.core.playback.session_transport(self.playback_id) {
            vixen_session::playback::Transport::Stopped => TransportState::Stopped,
            vixen_session::playback::Transport::Playing => TransportState::Playing,
            vixen_session::playback::Transport::Paused => TransportState::Paused,
        }
    }

    /// True once after playback reaches its end.
    pub fn take_finished(&self) -> bool {
        self.core.playback.take_session_finished(self.playback_id)
    }

    pub fn set_speed(&self, speed: f32) {
        self.core.playback.set_session_speed(self.playback_id, speed);
    }

    // Undo

    pub fn undo(&self, steps: u32) -> u32 {
        self.session.lock().undo(steps as usize) as u32
    }

    pub fn redo(&self, steps: u32) -> u32 {
        self.session.lock().redo(steps as usize) as u32
    }

    /// Descriptions, newest first, for the Edit menu and the toolbar's list.
    pub fn undo_descriptions(&self) -> Vec<String> {
        self.session.lock().undo_stack().undo_descriptions()
    }

    pub fn redo_descriptions(&self) -> Vec<String> {
        self.session.lock().undo_stack().redo_descriptions()
    }
}
