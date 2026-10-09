//! Worker event routing on the GTK thread.
use super::*;

impl Ui {
    pub(super) fn event(self: &Rc<Self>, event: Event) {
        match event {
            Event::Committed {
                generation,
                message,
                force_refresh,
            } if generation == self.model.borrow().generation && self.model.borrow().busy() => {
                self.finish();
                self.notice(&message, false);
                self.refresh(force_refresh);
            }
            Event::RefreshProgress {
                generation,
                completed,
                total,
            } => {
                if self
                    .model
                    .borrow_mut()
                    .progress(generation, completed, total)
                {
                    self.render_feedback();
                }
            }
            Event::Cards {
                generation,
                cards,
                terminal,
            } => {
                if self.model.borrow_mut().apply(generation, cards, terminal) {
                    self.render();
                }
            }
            Event::Done { generation, result } if generation == self.model.borrow().generation => {
                self.finish();
                match result {
                    Ok(message) => {
                        if !message.is_empty() {
                            self.notice(&message, false);
                        }
                    }
                    Err(message) => self.notice(&message, true),
                }
            }
            Event::Doctor {
                generation,
                lines,
                issues,
            } if generation == self.model.borrow().generation => {
                self.finish();
                if !self.closing.get() {
                    dialogs::doctor(&self.window, &lines, issues);
                }
            }
            Event::LoginStarted {
                generation,
                url,
                target_email,
            } if generation == self.model.borrow().generation => {
                if self.closing.get() {
                    return;
                }
                let commands = self.worker.commands.clone();
                let cancel = self.worker.commands.clone();
                let dialog = dialogs::login(
                    &self.window,
                    url,
                    target_email.as_deref(),
                    Rc::new(move |url| {
                        let _ = commands.send(Command::Callback(url));
                    }),
                    Rc::new(move || {
                        let _ = cancel.send(Command::CancelLogin);
                    }),
                );
                *self.login.borrow_mut() = Some(dialog);
            }
            Event::LoginPhase {
                generation,
                exchanging,
            } if generation == self.model.borrow().generation => {
                if let Some(login) = self.login.borrow().as_ref() {
                    login.status.set_text(if exchanging {
                        "正在交换并保存登录，请稍候…"
                    } else {
                        "等待浏览器回调…"
                    });
                    login.dialog.set_can_close(!exchanging);
                    login.cancel.set_sensitive(!exchanging);
                    login.submit.set_sensitive(!exchanging);
                    login.callback.set_sensitive(!exchanging);
                }
            }
            Event::CallbackRejected => {
                if let Some(login) = self.login.borrow().as_ref() {
                    login
                        .status
                        .set_text("回调无效，请确认地址完整且属于本次登录");
                } else {
                    self.notice("回调无效，请确认地址完整且属于本次登录", true);
                }
            }
            _ => {}
        }
    }
}
