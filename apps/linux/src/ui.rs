//! GTK main-thread controller. No credentials or blocking Core operations.
mod desktop;
mod events;
mod feedback;
mod setup;
use crate::{
    card::{Action, CardWidgets},
    dialogs::{self, LoginDialog},
    model::{Model, Operation},
    worker::{Command, Event, Worker},
};
use adw::prelude::*;
use gtk::gio;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

pub struct Ui {
    window: adw::ApplicationWindow,
    model: RefCell<Model>,
    worker: Worker,
    flow: gtk::FlowBox,
    cards: RefCell<HashMap<String, CardWidgets>>,
    empty: adw::StatusPage,
    banner: adw::Banner,
    feedback: gtk::Box,
    activity_label: gtk::Label,
    spinner: gtk::Spinner,
    toasts: adw::ToastOverlay,
    refresh_action: gio::SimpleAction,
    add_action: gio::SimpleAction,
    doctor_action: gio::SimpleAction,
    cancel_button: gtk::Button,
    login: RefCell<Option<LoginDialog>>,
    closing: Cell<bool>,
    next_refresh: Cell<Instant>,
    tray: crate::tray::Tray,
    tray_available: Cell<bool>,
    wake_pending: Cell<bool>,
    wake_subscription: RefCell<Option<gio::SignalSubscription>>,
}

impl Ui {
    pub fn present(&self) {
        self.window.unminimize();
        self.window.present();
    }

    pub fn refresh(self: &Rc<Self>, force: bool) {
        if self.model.borrow().busy() || self.closing.get() {
            return;
        }
        let preserve_error = !force && self.banner.is_revealed();
        self.worker.cancel.store(false, Ordering::Release);
        self.start(Operation::Refresh, |generation| Command::Refresh {
            generation,
            force,
        });
        if preserve_error {
            self.banner.set_revealed(true);
        }
    }

    fn start(self: &Rc<Self>, operation: Operation, make: impl FnOnce(u64) -> Command) {
        if self.closing.get() {
            return;
        }
        let generation = self.model.borrow_mut().begin(operation);
        if let Some(generation) = generation {
            self.banner.set_revealed(false);
            let command = make(generation);
            if let Some(activity) = self.model.borrow_mut().activity.as_mut() {
                activity.target_account = match &command {
                    Command::Switch { id, .. } | Command::Delete { id, .. } => Some(id.clone()),
                    Command::Login { target, .. } => target.clone(),
                    _ => None,
                };
            }
            if self.worker.commands.send(command).is_err() {
                self.model.borrow_mut().activity = None;
                self.notice("后台任务已停止，请重新启动应用", true);
            }
            self.render();
        }
    }

    fn start_login(self: &Rc<Self>, target: Option<String>) {
        let operation = if target.is_some() {
            Operation::Reauth
        } else {
            Operation::Add
        };
        self.start(operation, |generation| Command::Login {
            generation,
            target,
        });
    }

    fn account_action(self: &Rc<Self>, action: Action) {
        match action {
            Action::Switch(id) => self.start(Operation::Switch, |generation| Command::Switch {
                generation,
                id,
            }),
            Action::Reauth(id) => self.start_login(Some(id)),
            Action::Delete(id) => {
                let email = self
                    .model
                    .borrow()
                    .cards
                    .iter()
                    .find(|c| c.account.id == id)
                    .map(|c| c.account.email.clone());
                if let Some(email) = email {
                    let weak = Rc::downgrade(self);
                    dialogs::confirm_delete(&self.window, &email, move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.start(Operation::Delete, |generation| Command::Delete {
                                generation,
                                id: id.clone(),
                            });
                        }
                    });
                }
            }
        }
    }

    fn finish(self: &Rc<Self>) {
        self.model.borrow_mut().activity = None;
        self.cancel_button.set_visible(false);
        if let Some(login) = self.login.borrow_mut().take() {
            login.dialog.force_close();
        }
        self.render();
    }

    fn render(self: &Rc<Self>) {
        let model = self.model.borrow();
        let busy = model.busy() || self.closing.get();
        self.refresh_action.set_enabled(!busy);
        self.add_action.set_enabled(!busy);
        self.doctor_action.set_enabled(!busy);
        self.empty.set_visible(model.cards.is_empty());
        self.flow.set_visible(!model.cards.is_empty());
        self.render_feedback();
        self.tray.update(&model, self.closing.get());
        let mut widgets = self.cards.borrow_mut();
        widgets.retain(|id, widget| {
            let keep = model.cards.iter().any(|c| &c.account.id == id);
            if !keep {
                self.flow.remove(&widget.child);
            }
            keep
        });
        for card in &model.cards {
            let widget = widgets.entry(card.account.id.clone()).or_insert_with(|| {
                let weak = Rc::downgrade(self);
                let widget = CardWidgets::new(
                    &card.account.id,
                    Rc::new(move |action| {
                        if let Some(ui) = weak.upgrade() {
                            ui.account_action(action);
                        }
                    }),
                );
                self.flow.insert(&widget.child, -1);
                widget
            });
            let operation = model.activity.as_ref().and_then(|activity| {
                (activity.target_account.as_deref() == Some(card.account.id.as_str()))
                    .then_some(activity.operation)
            });
            widget.update(card, busy, operation);
        }
        let order: Vec<_> = model
            .cards
            .iter()
            .filter_map(|card| widgets.get(&card.account.id).map(|w| w.child.clone()))
            .collect();
        self.flow.set_sort_func(move |a, b| {
            order
                .iter()
                .position(|child| child == a)
                .cmp(&order.iter().position(|child| child == b))
                .into()
        });
    }

    fn notice(&self, message: &str, error: bool) {
        if self.closing.get() {
            return;
        }
        if error {
            if !self.window.is_visible() {
                self.present();
            }
            self.banner.set_title(message);
            self.banner.set_revealed(true);
        } else {
            self.toasts.add_toast(adw::Toast::new(message));
        }
    }

    fn shutdown(self: &Rc<Self>) {
        if self.closing.replace(true) {
            return;
        }
        self.wake_subscription.borrow_mut().take();
        self.worker.cancel.store(true, Ordering::Release);
        let _ = self.worker.commands.send(Command::Shutdown);
        self.banner.set_title("正在结束任务并保存结果，完成后退出…");
        self.banner.set_revealed(true);
        self.render();
    }
}

#[cfg(test)]
mod tests;
