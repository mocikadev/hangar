//! Native adaptive dialogs; callback URLs are never logged or persisted.
use adw::prelude::*;
use gtk::Orientation;
use std::rc::Rc;

pub struct LoginDialog {
    pub dialog: adw::Dialog,
    pub status: gtk::Label,
    pub callback: gtk::Entry,
    pub submit: gtk::Button,
    pub cancel: gtk::Button,
}

pub fn login(
    parent: &adw::ApplicationWindow,
    url: String,
    target_email: Option<&str>,
    submit_callback: Rc<dyn Fn(String)>,
    cancel_callback: Rc<dyn Fn()>,
) -> LoginDialog {
    let dialog = adw::Dialog::builder()
        .title(login_title(target_email))
        .content_width(480)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let body = gtk::Box::new(Orientation::Vertical, 16);
    margins(&body, 24);
    let explanation = gtk::Label::builder()
        .label(match target_email {
            Some(email) => format!(
                "请在浏览器中登录 {}，完成后更新此账号。保存不会自动切换。",
                crate::card::clean(email)
            ),
            None => "在浏览器中完成登录。保存账号不会自动切换。".into(),
        })
        .wrap(true)
        .xalign(0.0)
        .build();
    let browser = gtk::Button::with_label("在浏览器中打开");
    browser.add_css_class("suggested-action");
    let status = gtk::Label::builder()
        .label("等待浏览器回调…")
        .wrap(true)
        .xalign(0.0)
        .build();
    let fallback = gtk::Label::builder()
        .label("浏览器无法返回时，粘贴完整回调地址：")
        .wrap(true)
        .xalign(0.0)
        .build();
    let callback = gtk::Entry::builder()
        .placeholder_text("http://localhost:1455/auth/callback?…")
        .width_chars(1)
        .max_width_chars(28)
        .visibility(false)
        .input_purpose(gtk::InputPurpose::Password)
        .build();
    callback.update_property(&[gtk::accessible::Property::Label("完整 OAuth 回调地址")]);
    let submit = gtk::Button::with_label("提交回调");
    let cancel = gtk::Button::with_label("取消登录");
    let entry = callback.clone();
    submit.connect_clicked(move |_| {
        let value = entry.text().to_string();
        entry.set_text("");
        submit_callback(value);
    });
    let weak_status = status.downgrade();
    let weak_parent = parent.downgrade();
    browser.connect_clicked(move |_| {
        let launcher = gtk::UriLauncher::new(&url);
        let weak_status = weak_status.clone();
        launcher.launch(
            weak_parent.upgrade().as_ref(),
            None::<&gtk::gio::Cancellable>,
            move |result| {
                if result.is_err() {
                    if let Some(status) = weak_status.upgrade() {
                        status.set_text("无法打开浏览器，请检查默认浏览器设置后重试");
                    }
                }
            },
        );
    });
    let on_cancel = cancel_callback.clone();
    cancel.connect_clicked(move |_| on_cancel());
    dialog.connect_closed(move |_| cancel_callback());
    body.append(&explanation);
    body.append(&browser);
    body.append(&status);
    body.append(&gtk::Separator::new(Orientation::Horizontal));
    body.append(&fallback);
    body.append(&callback);
    body.append(&submit);
    body.append(&cancel);
    toolbar.set_content(Some(&body));
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));
    LoginDialog {
        dialog,
        status,
        callback,
        submit,
        cancel,
    }
}

pub fn doctor(parent: &adw::ApplicationWindow, lines: &[String], issues: usize) {
    let dialog = adw::Dialog::builder()
        .title("自检结果")
        .content_width(640)
        .content_height(520)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let body = gtk::Box::new(Orientation::Vertical, 16);
    margins(&body, 24);
    body.append(&crate::card::label(
        &format!("发现 {issues} 项问题"),
        "title-2",
    ));
    let text = gtk::Label::builder()
        .label(crate::card::clean(&lines.join("\n")))
        .wrap(true)
        .selectable(true)
        .xalign(0.0)
        .build();
    body.append(&text);
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&body)
        .build();
    toolbar.set_content(Some(&scroll));
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));
}

pub fn confirm_delete(parent: &adw::ApplicationWindow, email: &str, confirm: impl Fn() + 'static) {
    let dialog = adw::AlertDialog::builder()
        .heading("删除账号？")
        .body(format!(
            "将从 Hangar 移除 {}。再次添加需要重新登录。",
            crate::card::clean(email)
        ))
        .build();
    dialog.add_responses(&[("cancel", "取消"), ("delete", "删除账号")]);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
    dialog.connect_response(Some("delete"), move |_, _| confirm());
    dialog.present(Some(parent));
}

pub fn margins(widget: &impl IsA<gtk::Widget>, amount: i32) {
    widget.set_margin_start(amount);
    widget.set_margin_end(amount);
    widget.set_margin_top(amount);
    widget.set_margin_bottom(amount);
}

fn login_title(target_email: Option<&str>) -> String {
    target_email.map_or_else(
        || "添加账号".into(),
        |email| format!("重新登录 · {}", crate::card::clean(email)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn login_titles_distinguish_add_and_targeted_reauthentication() {
        assert_eq!(login_title(None), "添加账号");
        assert_eq!(
            login_title(Some("target@example.invalid")),
            "重新登录 · target@example.invalid"
        );
    }
}
