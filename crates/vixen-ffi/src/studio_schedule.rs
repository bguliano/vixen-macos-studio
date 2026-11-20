//! Studio: the show window's Schedule pane. Its edits are display commands (`studio_display_command`), shared with agents;
//! this adds the pane's list.
use crate::core::VixenCore;

#[uniffi::export]
impl VixenCore {
    /// The pane's snapshot as JSON: `{revision, liveOutput, schedulerOn, status, now, shows: [{id, name, schedules, sections: {startup,
    /// sequential, background, shutdown: [{action, name, sequence?, missing?, pauseSeconds?, program?}]}}], schedules: [{index,
    /// showId, showName, enabled, startDate, endDate, startTime, endTime, days, state, nextRun}], sequences}`.
    pub fn studio_schedule_list(&self) -> String {
        vixen_session::studio_schedule::list(&self.inner).to_string()
    }
}
