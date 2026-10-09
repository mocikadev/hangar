//! Optional SNI transport. No GTK, credentials, account I/O or business operations.
mod menu;
use crate::model::Model;
use ksni::blocking::TrayMethods;
use std::{
    cell::RefCell,
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Show,
    Refresh,
    Switch(String),
    Quit,
}

pub enum Event {
    Available(bool),
    Action(Action),
}

#[derive(Clone, Default, PartialEq, Eq)]
struct View {
    current: String,
    accounts: Vec<menu::AccountItem>,
    busy: bool,
}

impl View {
    fn from_model(model: &Model, closing: bool) -> Self {
        let busy = model.busy() || closing;
        Self {
            current: model
                .cards
                .iter()
                .find(|c| c.account.current)
                .map(|c| format!("当前：{}", menu::label(&c.account.email)))
                .unwrap_or_else(|| "尚未选择账号".into()),
            accounts: model
                .cards
                .iter()
                .map(|c| menu::AccountItem::new(c, busy))
                .collect(),
            busy,
        }
    }
}

enum Command {
    Update(View),
    Stop,
}

pub struct Tray {
    commands: mpsc::Sender<Command>,
    pub events: mpsc::Receiver<Event>,
    last: RefCell<Option<View>>,
}

impl Tray {
    pub fn start(icon: Vec<ksni::Icon>) -> Self {
        let (commands, receiver) = mpsc::channel();
        let (sender, events) = mpsc::channel();
        std::thread::spawn(move || run(receiver, sender, icon));
        Self {
            commands,
            events,
            last: RefCell::new(None),
        }
    }

    pub fn update(&self, model: &Model, closing: bool) {
        let view = View::from_model(model, closing);
        if self.last.borrow().as_ref() != Some(&view) {
            let _ = self.commands.send(Command::Update(view.clone()));
            *self.last.borrow_mut() = Some(view);
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
    }
}

#[derive(Clone)]
struct Item {
    view: View,
    events: mpsc::Sender<Event>,
    icon: Vec<ksni::Icon>,
}

impl Item {
    fn send(&self, action: Action) {
        let _ = self.events.send(Event::Action(action));
    }
}

impl ksni::Tray for Item {
    fn id(&self) -> String {
        "dev.mocika.hangar".into()
    }
    fn title(&self) -> String {
        "Hangar".into()
    }
    fn icon_name(&self) -> String {
        if self.icon.is_empty() {
            "avatar-default-symbolic".into()
        } else {
            String::new()
        }
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icon.clone()
    }
    fn activate(&mut self, _: i32, _: i32) {
        self.send(Action::Show);
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        menu::build(self)
    }
    fn watcher_offline(&self, _: ksni::OfflineReason) -> bool {
        let _ = self.events.send(Event::Available(false));
        // Recreate on our background thread; the UI immediately restores a hidden window.
        false
    }
}

fn run(receiver: mpsc::Receiver<Command>, events: mpsc::Sender<Event>, icon: Vec<ksni::Icon>) {
    let mut item = Item {
        view: View::default(),
        events: events.clone(),
        icon,
    };
    let mut handle: Option<ksni::blocking::Handle<Item>> = None;
    let mut retry_at = Instant::now();
    loop {
        if handle.as_ref().is_some_and(|h| h.is_closed()) {
            handle = None;
            let _ = events.send(Event::Available(false));
        }
        if handle.is_none() && Instant::now() >= retry_at {
            if let Ok(started) = item.clone().spawn() {
                handle = Some(started);
                let _ = events.send(Event::Available(true));
            }
            retry_at = Instant::now() + Duration::from_secs(2);
        }
        match receiver.recv_timeout(Duration::from_millis(250)) {
            Ok(Command::Update(mut view)) => {
                // Coalesce snapshots; never block the GTK thread waiting for D-Bus.
                while let Ok(command) = receiver.try_recv() {
                    match command {
                        Command::Update(next) => view = next,
                        Command::Stop => {
                            if let Some(h) = handle {
                                h.shutdown().wait();
                            }
                            return;
                        }
                    }
                }
                item.view = view;
                if let Some(h) = &handle {
                    h.update(|tray| tray.view = item.view.clone());
                }
            }
            Ok(Command::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                if let Some(h) = handle {
                    h.shutdown().wait();
                }
                return;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}
