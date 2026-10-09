//! Render task feedback without rebuilding cards or changing keyboard focus.
use super::*;

impl Ui {
    pub(super) fn render_feedback(&self) {
        let model = self.model.borrow();
        let activity = model.activity.as_ref();
        // OAuth has its own native dialog and phase feedback.
        let visible = !self.closing.get()
            && self.login.borrow().is_none()
            && activity.is_some_and(|a| a.visible_at(Instant::now()));
        self.feedback.set_visible(visible);
        self.spinner.set_spinning(visible);
        let message = if visible {
            activity.map(|a| a.message()).unwrap_or_default()
        } else {
            String::new()
        };
        if self.activity_label.text() != message {
            self.activity_label.set_text(&message);
        }
        self.cancel_button
            .set_visible(visible && activity.is_some_and(|a| a.operation == Operation::Refresh));
        self.cancel_button
            .set_sensitive(!self.closing.get() && activity.is_some_and(|a| a.can_stop()));
    }

    pub(super) fn stop_refresh(&self) {
        let accepted = self
            .model
            .borrow_mut()
            .activity
            .as_mut()
            .is_some_and(|a| a.stop_refresh());
        if accepted {
            self.worker.cancel.store(true, Ordering::Release);
            self.render_feedback();
        }
    }
}
