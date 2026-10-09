use super::*;
use crate::model::{Card, QuotaState};
use gtk::glib;
use hangar_core::summary::{
    AccessTokenSummaryState, AccountSummary, QuotaSummary, TokenHealthSummary,
};

fn iterate() {
    for _ in 0..20 {
        while glib::MainContext::default().iteration(false) {}
        std::thread::sleep(Duration::from_millis(20));
    }
}

// Pure presentation fixtures: no Account, token, network or account file.
// Fixed quota timestamps below are UI layout examples, not service results.
// This checks eight-card layout only, never real-account acceptance.
fn fixture(index: usize) -> Card {
    Card {
        account: AccountSummary {
            id: index.to_string(),
            email: format!("layout-{index}@example.invalid"),
            current: index == 0,
            stale: index == 7,
            account_id: None,
            organization_id: None,
            expires_at: None,
            token_health: TokenHealthSummary {
                status: AccessTokenSummaryState::Unknown,
                expiry_source: None,
                refresh_available: false,
            },
        },
        quota: (index < 6).then_some(QuotaSummary {
            plan: Some("展示夹具".into()),
            weekly_remaining: Some(90 - index as i32 * 10),
            weekly_reset_at: Some(1_791_580_200),
            reset_available: Some(if index == 2 { 2 } else { 0 }),
            reset_next_expiry: (index == 2).then_some(1_791_580_200),
        }),
        fetched_at: (index < 6).then_some(1_791_504_000),
        state: match index {
            4 => QuotaState::Old,
            5 => QuotaState::Failed,
            6.. => QuotaState::Unknown,
            _ => QuotaState::Fresh,
        },
        recommended: index == 1,
    }
}

fn screenshot(ui: &Ui, name: &str) {
    let Some(dir) = std::env::var_os("HANGAR_LAYOUT_OUTPUT") else {
        return;
    };
    let paintable = gtk::WidgetPaintable::new(Some(&ui.window));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(
        &snapshot,
        f64::from(ui.window.width()),
        f64::from(ui.window.height()),
    );
    let node = snapshot
        .to_node()
        .unwrap_or_else(|| panic!("mapped window render node for {name}"));
    let texture = ui.window.renderer().unwrap().render_texture(&node, None);
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    texture.save_to_png(dir.join(name)).unwrap();
}

#[test]
#[ignore = "GTK layout test: isolated HOME and display required; run with --ignored --test-threads=1"]
fn eight_card_layout_adapts_to_narrow_windows_and_system_theme() {
    let app = adw::Application::builder()
        .application_id("dev.mocika.hangar.LayoutTest")
        .build();
    app.register(None::<&gio::Cancellable>).unwrap();
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let ui = Ui::new(&app);
    crate::card::tests::verify_card_states(fixture(0));
    // Prevent real Core calls through activation while using UI-only summaries.
    ui.model.borrow_mut().begin(Operation::Refresh);
    ui.model.borrow_mut().cards = (0..8).map(fixture).collect();
    ui.render();
    ui.present();
    iterate();
    assert_eq!(ui.cards.borrow().len(), 8);
    let widgets = ui.cards.borrow();
    let first = &widgets["0"].child;
    assert!(first.width() >= 280);
    let second = &widgets["1"].child;
    let a = first.compute_bounds(&ui.flow).unwrap();
    let b = second.compute_bounds(&ui.flow).unwrap();
    assert_eq!(a.y(), b.y(), "wide layout must show multiple columns");
    drop(widgets);
    assert!(ui.flow.child_focus(gtk::DirectionType::TabForward));
    let focused = gtk::prelude::RootExt::focus(&ui.window);
    assert!(focused.is_some());
    ui.render();
    assert_eq!(
        gtk::prelude::RootExt::focus(&ui.window),
        focused,
        "refreshing existing cards must preserve keyboard focus"
    );
    iterate();
    screenshot(&ui, "layout-wide.png");
    ui.window.set_default_size(360, 800);
    iterate();
    let widgets = ui.cards.borrow();
    let first = widgets["0"].child.compute_bounds(&ui.flow).unwrap();
    let second = widgets["1"].child.compute_bounds(&ui.flow).unwrap();
    assert_eq!(first.x(), second.x(), "narrow layout must have one column");
    assert!(
        ui.window.width() <= 380,
        "content must not force a wide minimum"
    );
    drop(widgets);
    screenshot(&ui, "layout-narrow.png");
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
    iterate();
    screenshot(&ui, "layout-dark.png");
    // Presentation-only events validate feedback without inventing credentials or requests.
    let generation = ui.model.borrow().generation;
    ui.event(Event::RefreshProgress {
        generation,
        completed: 1,
        total: 3,
    });
    iterate();
    assert!(ui.feedback.is_visible());
    assert!(ui.spinner.is_spinning());
    assert!(ui.activity_label.text().contains("1/3"));
    let focused = gtk::prelude::RootExt::focus(&ui.window);
    ui.event(Event::RefreshProgress {
        generation,
        completed: 2,
        total: 3,
    });
    assert_eq!(gtk::prelude::RootExt::focus(&ui.window), focused);
    iterate();
    screenshot(&ui, "feedback-refresh.png");
    ui.cancel_button.emit_clicked();
    assert!(ui.activity_label.text().contains("正在停止"));
    assert!(!ui.cancel_button.is_sensitive());
    ui.event(Event::RefreshProgress {
        generation,
        completed: 3,
        total: 3,
    });
    assert!(ui.activity_label.text().contains("正在停止"));
    iterate();
    assert!(
        ui.window.width() <= 380,
        "stopping feedback must wrap in narrow windows"
    );
    screenshot(&ui, "feedback-stopping.png");
    ui.event(Event::Done {
        generation,
        result: Err("展示测试：任务失败".into()),
    });
    assert!(!ui.spinner.is_spinning());
    assert!(!ui.feedback.is_visible());
    assert!(!ui.cancel_button.is_visible());
    assert!(ui.banner.is_revealed());
    ui.event(Event::RefreshProgress {
        generation,
        completed: 3,
        total: 3,
    });
    assert!(!ui.feedback.is_visible());
    assert!(!ui.model.borrow().busy());
    for operation in [Operation::Switch, Operation::Delete, Operation::Reauth] {
        let generation = ui.model.borrow_mut().begin(operation).unwrap();
        ui.model
            .borrow_mut()
            .activity
            .as_mut()
            .unwrap()
            .target_account = Some("1".into());
        ui.render();
        crate::card::tests::assert_operation_visible(&ui.cards.borrow()["1"], true);
        crate::card::tests::assert_operation_visible(&ui.cards.borrow()["0"], false);
        iterate();
        if operation == Operation::Switch {
            screenshot(&ui, "card-switching.png");
        }
        ui.event(Event::Done {
            generation,
            result: Ok(String::new()),
        });
        assert!(ui.model.borrow().activity.is_none());
        crate::card::tests::assert_operation_visible(&ui.cards.borrow()["1"], false);
    }
    // Show independent reset information even without a weekly value, plus error/stale states.
    let mut partial = fixture(2);
    partial.quota.as_mut().unwrap().weekly_remaining = None;
    let mut failed = fixture(6);
    failed.state = QuotaState::Failed;
    let mut stale = fixture(7);
    stale.account.current = true;
    ui.model.borrow_mut().cards = vec![stale, partial, failed];
    ui.window.set_default_size(1180, 800);
    ui.render();
    iterate();
    screenshot(&ui, "card-states-dark.png");
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceLight);
    iterate();
    screenshot(&ui, "card-states-light.png");
    // Lifecycle projection: a registered tray keeps the window recoverable.
    // These events do not claim the desktop host itself was tested here.
    ui.model.borrow_mut().begin(Operation::Doctor);
    ui.tray_event(crate::tray::Event::Available(true));
    ui.close_window();
    assert!(!ui.window.is_visible());
    assert!(!ui.closing.get());
    ui.tray_event(crate::tray::Event::Available(false));
    assert!(ui.window.is_visible());
    assert!(!ui.closing.get());
    // Exercise real empty-home Core I/O and native dialogs on the same GTK thread.
    // Fixtures are replaced by the actual empty snapshot before operations.
    assert!(hangar_core::load_accounts().unwrap().accounts.is_empty());
    ui.model.borrow_mut().activity = None;
    ui.refresh(false);
    assert!(
        ui.banner.is_revealed(),
        "automatic checks must not erase an existing error"
    );
    iterate();
    assert!(!ui.model.borrow().busy());
    assert!(ui.model.borrow().cards.is_empty());
    ui.start(Operation::Doctor, |generation| Command::Doctor {
        generation,
    });
    iterate();
    let doctor = ui.window.visible_dialog().expect("native doctor dialog");
    assert_eq!(doctor.title(), "自检结果");
    println!(
        "doctor allocation {}x{} mapped={}, parent {}x{} mapped={} visible={}",
        doctor.width(),
        doctor.height(),
        doctor.is_mapped(),
        ui.window.width(),
        ui.window.height(),
        ui.window.is_mapped(),
        ui.window.is_visible()
    );
    screenshot(&ui, "dialog-doctor.png");
    assert!(doctor.close());
    iterate();
    assert!(ui.window.visible_dialog().is_none());
    for _ in 0..2 {
        ui.start_login(None);
        iterate();
        let login = ui.login.borrow();
        let dialog = login.as_ref().expect("native OAuth dialog");
        assert!(dialog.cancel.is_sensitive());
        assert_eq!(dialog.dialog.title(), "添加账号");
        screenshot(&ui, "dialog-oauth.png");
        dialog
            .callback
            .set_text("http://localhost:1455/auth/callback?state=wrong&code=invalid");
        dialog.submit.emit_clicked();
        drop(login);
        iterate();
        assert!(ui
            .login
            .borrow()
            .as_ref()
            .unwrap()
            .status
            .text()
            .contains("回调无效"));
        iterate();
        assert!(
            ui.login
                .borrow()
                .as_ref()
                .unwrap()
                .status
                .text()
                .contains("回调无效"),
            "unchanged waiting phase must not erase useful error feedback"
        );
        ui.login.borrow().as_ref().unwrap().cancel.emit_clicked();
        iterate();
        assert!(ui.login.borrow().is_none());
        assert!(!ui.model.borrow().busy());
    }
    // A committed event schedules refresh; ordinary cancelled/failed Done does not.
    // Injected completion tests scheduling only, never a real credential save.
    let generation = ui.model.borrow_mut().begin(Operation::Switch).unwrap();
    ui.event(Event::Committed {
        generation,
        message: String::new(),
        force_refresh: false,
    });
    assert_eq!(ui.model.borrow().generation, generation + 1);
    iterate();
    assert!(!ui.model.borrow().busy());
    let generation = ui.model.borrow_mut().begin(Operation::Add).unwrap();
    ui.wake_pending.set(true);
    iterate();
    assert_eq!(
        ui.model.borrow().generation,
        generation,
        "wake waits for current operation"
    );
    ui.event(Event::Done {
        generation,
        result: Ok("已取消登录".into()),
    });
    assert_eq!(
        ui.model.borrow().generation,
        generation,
        "cancel alone does not refresh"
    );
    iterate();
    assert_eq!(
        ui.model.borrow().generation,
        generation + 1,
        "deferred wake refreshes once"
    );
    iterate();
    assert!(!ui.wake_pending.get());
    // Native close-request follows the same graceful worker shutdown as Ctrl+Q.
    ui.window.close();
    iterate();
    assert!(ui.closing.get());
    assert!(!ui.window.is_visible());
}
