//! Desktop integration stays on the GTK thread; SNI work runs in the tray adapter.
use super::*;
use crate::tray;

impl Ui {
    pub(super) fn close_window(self: &Rc<Self>) {
        if self.closing.get() {
            return;
        }
        if self.tray_available.get() {
            self.window.set_visible(false);
        } else {
            self.shutdown();
        }
    }

    pub(super) fn tray_event(self: &Rc<Self>, event: tray::Event) {
        if self.closing.get() {
            return;
        }
        match event {
            tray::Event::Available(available) => {
                self.tray_available.set(available);
                if !available && !self.window.is_visible() {
                    self.present();
                }
            }
            tray::Event::Action(action) => match action {
                tray::Action::Show => self.present(),
                tray::Action::Refresh => self.refresh(true),
                tray::Action::Quit => self.shutdown(),
                tray::Action::Switch(id) => {
                    let allowed = {
                        let model = self.model.borrow();
                        !model.busy()
                            && model.cards.iter().any(|c| {
                                c.account.id == id && !c.account.current && !c.account.stale
                            })
                    };
                    if allowed {
                        self.account_action(Action::Switch(id));
                    }
                }
            },
        }
    }

    pub(super) fn connect_wake(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        gio::bus_get(
            gio::BusType::System,
            None::<&gio::Cancellable>,
            move |result| {
                let (Ok(bus), Some(ui)) = (result, weak.upgrade()) else {
                    return;
                };
                if ui.closing.get() {
                    return;
                }
                let weak = Rc::downgrade(&ui);
                let subscription = bus.subscribe_to_signal(
                    Some("org.freedesktop.login1"),
                    Some("org.freedesktop.login1.Manager"),
                    Some("PrepareForSleep"),
                    Some("/org/freedesktop/login1"),
                    None,
                    gio::DBusSignalFlags::NONE,
                    move |signal| {
                        if signal.parameters.get::<(bool,)>() == Some((false,)) {
                            if let Some(ui) = weak.upgrade() {
                                ui.wake_pending.set(true);
                            }
                        }
                    },
                );
                *ui.wake_subscription.borrow_mut() = Some(subscription);
            },
        );
    }

    pub(super) fn tray_icon() -> Vec<ksni::Icon> {
        let Ok(pixbuf) = gtk::gdk_pixbuf::Pixbuf::from_resource_at_scale(
            "/dev/mocika/hangar/icons/scalable/apps/dev.mocika.hangar.svg",
            32,
            32,
            true,
        ) else {
            return Vec::new();
        };
        let bytes = pixbuf.read_pixel_bytes();
        let channels = pixbuf.n_channels() as usize;
        let mut data = Vec::new();
        for y in 0..pixbuf.height() as usize {
            for x in 0..pixbuf.width() as usize {
                let start = y * pixbuf.rowstride() as usize + x * channels;
                let pixel = &bytes[start..start + channels];
                data.extend_from_slice(&[
                    if channels == 4 { pixel[3] } else { 255 },
                    pixel[0],
                    pixel[1],
                    pixel[2],
                ]);
            }
        }
        vec![ksni::Icon {
            width: pixbuf.width(),
            height: pixbuf.height(),
            data,
        }]
    }
}
