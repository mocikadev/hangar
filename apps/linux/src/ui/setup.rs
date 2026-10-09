//! Native window assembly, application actions and main-loop sources.
use super::*;
use gtk::{glib, Orientation};

impl Ui {
    pub fn new(app: &adw::Application) -> Rc<Self> {
        let css = gtk::CssProvider::new();
        css.load_from_string(
            ".account-card { padding: 18px; }
             .account-card.current-account { outline: 2px solid @accent_color; outline-offset: -2px; }
             flowboxchild { padding: 6px; }",
        );
        gtk::style_context_add_provider_for_display(
            &gtk::prelude::WidgetExt::display(&gtk::Box::new(Orientation::Vertical, 0)),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Hangar — 账号总览")
            .default_width(1180)
            .default_height(800)
            .build();
        let toolbar = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&adw::WindowTitle::new("Hangar", "账号总览")));
        let add = gtk::Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("添加账号 (Ctrl+N)")
            .action_name("app.add")
            .build();
        let refresh = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text("刷新周额度 (Ctrl+R)")
            .action_name("app.refresh")
            .build();
        let cancel_button = gtk::Button::builder()
            .icon_name("process-stop-symbolic")
            .tooltip_text("停止刷新，保留已完成的结果")
            .visible(false)
            .build();
        header.pack_start(&add);
        header.pack_end(&refresh);
        header.pack_end(&cancel_button);
        let menu = gio::Menu::new();
        menu.append(Some("自检"), Some("app.doctor"));
        menu.append(Some("关于 Hangar"), Some("app.about"));
        menu.append(Some("退出"), Some("app.quit"));
        header.pack_end(
            &gtk::MenuButton::builder()
                .icon_name("open-menu-symbolic")
                .tooltip_text("主菜单")
                .menu_model(&menu)
                .build(),
        );
        toolbar.add_top_bar(&header);
        let feedback = gtk::Box::new(Orientation::Horizontal, 8);
        dialogs::margins(&feedback, 12);
        feedback.set_visible(false);
        let spinner = gtk::Spinner::new();
        let activity_label = crate::card::label("", "");
        activity_label.set_wrap(true);
        activity_label.set_hexpand(true);
        feedback.append(&spinner);
        feedback.append(&activity_label);
        toolbar.add_top_bar(&feedback);
        let content = gtk::Box::new(Orientation::Vertical, 12);
        let banner = adw::Banner::new("");
        content.append(&banner);
        let flow = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .homogeneous(true)
            .min_children_per_line(1)
            .max_children_per_line(3)
            .row_spacing(6)
            .column_spacing(6)
            .valign(gtk::Align::Start)
            .build();
        let empty = adw::StatusPage::builder()
            .title("添加你的第一个账号")
            .description(
                "已有 Codex 登录会自动收敛到共享账号库。也可以添加账号，登录后再选择切换。",
            )
            .icon_name("avatar-default-symbolic")
            .build();
        let empty_add = gtk::Button::builder()
            .label("添加账号")
            .halign(gtk::Align::Center)
            .action_name("app.add")
            .css_classes(["suggested-action", "pill"])
            .build();
        empty.set_child(Some(&empty_add));
        content.append(&empty);
        content.append(&flow);
        let clamp = adw::Clamp::builder()
            .maximum_size(1200)
            .tightening_threshold(1000)
            .child(&content)
            .build();
        dialogs::margins(&clamp, 6);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&clamp)
            .build();
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&scroll));
        toolbar.set_content(Some(&toasts));
        window.set_content(Some(&toolbar));
        let ui = Rc::new(Self {
            window,
            model: RefCell::new(Model::default()),
            worker: Worker::start(),
            flow,
            cards: RefCell::new(HashMap::new()),
            empty,
            banner,
            feedback,
            activity_label,
            spinner,
            toasts,
            refresh_action: gio::SimpleAction::new("refresh", None),
            add_action: gio::SimpleAction::new("add", None),
            doctor_action: gio::SimpleAction::new("doctor", None),
            cancel_button,
            login: RefCell::new(None),
            closing: Cell::new(false),
            next_refresh: Cell::new(Instant::now() + Duration::from_secs(60)),
            tray: crate::tray::Tray::start(Self::tray_icon()),
            tray_available: Cell::new(false),
            wake_pending: Cell::new(false),
            wake_subscription: RefCell::new(None),
        });
        ui.connect(app);
        ui.connect_wake();
        ui
    }

    fn connect(self: &Rc<Self>, app: &adw::Application) {
        for (action, kind) in [
            (&self.refresh_action, "refresh"),
            (&self.add_action, "add"),
            (&self.doctor_action, "doctor"),
        ] {
            let weak = Rc::downgrade(self);
            action.connect_activate(move |_, _| {
                if let Some(ui) = weak.upgrade() {
                    match kind {
                        "refresh" => ui.refresh(true),
                        "add" => ui.start_login(None),
                        _ => ui.start(Operation::Doctor, |generation| Command::Doctor {
                            generation,
                        }),
                    }
                }
            });
            app.add_action(action);
        }
        let quit = gio::SimpleAction::new("quit", None);
        let weak = Rc::downgrade(self);
        quit.connect_activate(move |_, _| {
            if let Some(ui) = weak.upgrade() {
                ui.shutdown();
            }
        });
        app.add_action(&quit);
        let about = gio::SimpleAction::new("about", None);
        let parent = self.window.downgrade();
        about.connect_activate(move |_, _| {
            if let Some(parent) = parent.upgrade() {
                adw::AboutDialog::builder()
                    .application_name("Hangar")
                    .application_icon("dev.mocika.hangar")
                    .version(env!("CARGO_PKG_VERSION"))
                    .developer_name("Mocika")
                    .comments(
                        "Codex 多账号管理 · 状态栏可用时关闭窗口保留后台运行；菜单中可完全退出。",
                    )
                    .website("https://github.com/mocikadev/hangar")
                    .build()
                    .present(Some(&parent));
            }
        });
        app.add_action(&about);
        app.set_accels_for_action("app.refresh", &["<Primary>r"]);
        app.set_accels_for_action("app.add", &["<Primary>n"]);
        app.set_accels_for_action("app.doctor", &["<Primary><Shift>d"]);
        app.set_accels_for_action("app.quit", &["<Primary>q"]);
        let close = gio::SimpleAction::new("close", None);
        let weak = Rc::downgrade(self);
        close.connect_activate(move |_, _| {
            if let Some(ui) = weak.upgrade() {
                ui.close_window();
            }
        });
        app.add_action(&close);
        app.set_accels_for_action("app.close", &["<Primary>w"]);
        let weak = Rc::downgrade(self);
        self.window.connect_close_request(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.close_window();
            }
            glib::Propagation::Stop
        });
        let weak = Rc::downgrade(self);
        self.cancel_button.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.stop_refresh();
            }
        });
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(100), move || {
            let Some(ui) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            while let Ok(event) = ui.worker.events.try_recv() {
                if matches!(event, Event::Stopped) {
                    ui.window.destroy();
                    if let Some(app) = ui.window.application() {
                        app.quit();
                    }
                    return glib::ControlFlow::Break;
                }
                ui.event(event);
            }
            while let Ok(event) = ui.tray.events.try_recv() {
                ui.tray_event(event);
            }
            ui.render_feedback();
            if !ui.closing.get() && !ui.model.borrow().busy() && ui.wake_pending.replace(false) {
                ui.refresh(false);
            }
            if !ui.closing.get() && Instant::now() >= ui.next_refresh.get() {
                ui.next_refresh
                    .set(Instant::now() + Duration::from_secs(60));
                ui.refresh(false);
            }
            glib::ControlFlow::Continue
        });
        let weak = Rc::downgrade(self);
        self.window.connect_is_active_notify(move |window| {
            if window.is_active() {
                if let Some(ui) = weak.upgrade() {
                    ui.refresh(false);
                }
            }
        });
    }
}
