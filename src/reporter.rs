use std::sync::Arc;

use crossbeam_channel::Sender;

use crate::tui::UiEvent;

pub trait Reporter: Send + Sync {
    fn step(&self, title: String, body: String);
    fn update(&self, body: String);
    fn ok(&self, msg: String);
    fn error(&self, msg: String);
}

pub type DynReporter = Arc<dyn Reporter>;

#[derive(Clone, Default)]
pub struct PlainReporter;

impl PlainReporter {
    pub fn new() -> Self {
        Self
    }
}

impl Reporter for PlainReporter {
    fn step(&self, title: String, body: String) {
        eprintln!("==> {}", title);
        if !body.trim().is_empty() {
            eprintln!("{}", body);
        }
    }

    fn update(&self, body: String) {
        if !body.trim().is_empty() {
            eprintln!("{}", body);
        }
    }

    fn ok(&self, msg: String) {
        if msg.trim().is_empty() {
            eprintln!("OK");
        } else {
            eprintln!("OK: {}", msg);
        }
    }

    fn error(&self, msg: String) {
        eprintln!("ERROR: {}", msg);
    }
}

#[derive(Clone)]
pub struct ChannelReporter {
    tx: Sender<UiEvent>,
}

impl ChannelReporter {
    pub fn new(tx: Sender<UiEvent>) -> Self {
        Self { tx }
    }

    fn send(&self, ev: UiEvent) {
        // If the UI has exited, ignore further updates.
        let _ = self.tx.send(ev);
    }
}

impl Reporter for ChannelReporter {
    fn step(&self, title: String, body: String) {
        self.send(UiEvent::SetStep { title, body });
    }

    fn update(&self, body: String) {
        self.send(UiEvent::UpdateBody { body });
    }

    fn ok(&self, msg: String) {
        self.send(UiEvent::SetOk { msg });
    }

    fn error(&self, msg: String) {
        self.send(UiEvent::SetError { msg });
    }
}
