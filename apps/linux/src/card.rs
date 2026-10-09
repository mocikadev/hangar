//! Native account card; all supplied values are already credential-free.
mod presentation;
use crate::model::{Card, Operation, QuotaState};
use adw::prelude::*;
use gtk::{pango::EllipsizeMode, Orientation};
use std::{cell::Cell, rc::Rc};

#[derive(Clone)]
pub enum Action {
    Switch(String),
    Reauth(String),
    Delete(String),
}

pub struct CardWidgets {
    pub child: gtk::FlowBoxChild,
    body: gtk::Box,
    email: gtk::Label,
    plan: gtk::Label,
    quota: gtk::Box,
    badges: gtk::Label,
    remaining: gtk::Label,
    progress: gtk::ProgressBar,
    details: gtk::Label,
    operation: gtk::Label,
    state: gtk::Label,
    switch: gtk::Button,
    reauth: gtk::Button,
    delete: gtk::Button,
    more: gtk::MenuButton,
    needs_login: Rc<Cell<bool>>,
}

impl CardWidgets {
    pub fn new(id: &str, action: Rc<dyn Fn(Action)>) -> Self {
        let body = gtk::Box::new(Orientation::Vertical, 8);
        body.add_css_class("account-card");
        body.add_css_class("card");
        body.set_size_request(280, -1);
        let email = label("", "heading");
        email.set_ellipsize(EllipsizeMode::Middle);
        email.set_max_width_chars(28);
        let plan = label("套餐未知", "dim-label");
        let badges = label("", "caption-heading");
        // Reserve the status row so quota values align across the grid.
        badges.set_size_request(-1, 16);
        let quota = gtk::Box::new(Orientation::Vertical, 8);
        let remaining = label("未知", "title-1");
        let caption = label("周剩余额度", "dim-label");
        let progress = gtk::ProgressBar::new();
        let details = label("", "caption");
        details.set_wrap(true);
        let operation = label("", "accent");
        operation.set_wrap(true);
        let state = label("", "caption");
        state.set_wrap(true);
        let buttons = gtk::Box::new(Orientation::Horizontal, 8);
        let switch = gtk::Button::with_label("切换到此账号");
        switch.set_hexpand(true);
        switch.add_css_class("suggested-action");
        let reauth = gtk::Button::with_label("重新登录");
        let delete = gtk::Button::with_label("删除账号…");
        delete.add_css_class("destructive-action");
        let popover = gtk::Popover::new();
        let actions = gtk::Box::new(Orientation::Vertical, 6);
        actions.append(&reauth);
        actions.append(&delete);
        popover.set_child(Some(&actions));
        let more = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .popover(&popover)
            .build();
        let needs_login = Rc::new(Cell::new(false));
        let stale = needs_login.clone();
        let on_primary = action.clone();
        let account_id = id.to_string();
        switch.connect_clicked(move |_| {
            on_primary(if stale.get() {
                Action::Reauth(account_id.clone())
            } else {
                Action::Switch(account_id.clone())
            });
        });
        for (button, command) in [
            (&reauth, Action::Reauth(id.into())),
            (&delete, Action::Delete(id.into())),
        ] {
            let action = action.clone();
            let popover = popover.clone();
            button.connect_clicked(move |_| {
                popover.popdown();
                action(command.clone());
            });
        }
        buttons.set_valign(gtk::Align::End);
        buttons.set_vexpand(true);
        buttons.append(&switch);
        buttons.append(&more);
        for widget in [&email, &plan, &badges] {
            body.append(widget);
        }
        quota.append(&caption);
        quota.append(&remaining);
        quota.append(&progress);
        quota.append(&details);
        body.append(&quota);
        body.append(&state);
        body.append(&operation);
        body.append(&buttons);
        let child = gtk::FlowBoxChild::new();
        child.set_focusable(false);
        child.set_child(Some(&body));
        Self {
            child,
            body,
            email,
            plan,
            quota,
            badges,
            remaining,
            progress,
            details,
            operation,
            state,
            switch,
            reauth,
            delete,
            more,
            needs_login,
        }
    }

    pub fn update(&self, card: &Card, busy: bool, operation: Option<Operation>) {
        let account = &card.account;
        self.needs_login.set(account.stale);
        let email = clean(&account.email);
        self.email.set_text(&email);
        self.email.set_tooltip_text(Some(&email));
        self.plan.set_text(&clean(
            card.quota
                .as_ref()
                .and_then(|q| q.plan.as_deref())
                .unwrap_or("套餐未知"),
        ));
        self.plan.set_ellipsize(EllipsizeMode::End);
        self.plan.set_tooltip_text(Some(&self.plan.text()));
        let mut badges = Vec::new();
        if account.current {
            badges.push("✓ 使用中");
            self.body.add_css_class("current-account");
        } else {
            self.body.remove_css_class("current-account");
        }
        if card.recommended && !account.stale {
            badges.push("✦ 推荐");
        }
        self.badges.set_text(&badges.join(" · "));
        self.badges.add_css_class("accent");
        self.quota.set_visible(!account.stale);
        self.remaining
            .set_text(&card.remaining().map_or("未知".into(), |n| format!("{n}%")));
        self.progress
            .set_fraction(f64::from(card.remaining().unwrap_or(0).clamp(0, 100)) / 100.0);
        self.progress.set_visible(card.remaining().is_some());
        self.details.set_text(&presentation::quota_details(card));
        self.state.set_text(&presentation::cache_status(card));
        self.state.set_visible(!self.state.text().is_empty());
        for class in ["warning", "error", "dim-label"] {
            self.state.remove_css_class(class);
        }
        self.state
            .add_css_class(if account.stale || card.state == QuotaState::Old {
                "warning"
            } else if card.state == QuotaState::Failed {
                "error"
            } else {
                "dim-label"
            });
        self.operation.set_text(match operation {
            Some(Operation::Switch) => "正在切换到此账号…",
            Some(Operation::Delete) => "正在删除此账号…",
            Some(Operation::Reauth) => "正在重新登录此账号…",
            _ => "",
        });
        self.operation
            .set_visible(!self.operation.text().is_empty());
        self.switch.set_label(if account.stale {
            "重新登录"
        } else {
            "切换到此账号"
        });
        self.switch.set_visible(!account.current || account.stale);
        self.switch
            .set_sensitive(!busy && (!account.current || account.stale));
        self.reauth.set_visible(!account.stale);
        self.reauth.set_sensitive(!busy);
        self.delete.set_visible(!account.current);
        self.delete.set_sensitive(!busy && !account.current);
        self.more.set_visible(!account.stale || !account.current);
        self.more.set_halign(gtk::Align::End);
        self.more.set_hexpand(account.current && !account.stale);
        // Keep the menu focusable during work; mutation items carry the busy guard.
        let more_label = format!("更多账号操作，{email}");
        self.more.set_tooltip_text(Some(&more_label));
        self.more
            .update_property(&[gtk::accessible::Property::Label(&more_label)]);
        self.switch
            .update_property(&[gtk::accessible::Property::Label(&format!(
                "{}，{email}",
                self.switch.label().unwrap_or_default()
            ))]);
    }
}

pub fn clean(text: &str) -> String {
    text.replace('\0', "�")
}
pub fn label(text: &str, class: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .css_classes([class])
        .build()
}

#[cfg(test)]
pub(crate) mod tests;
