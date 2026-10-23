//! Background work that reports back to the interface. A task resolves to an `Update`, which runs on
//! the UI thread at the start of the next frame with mutable access to the whole app.

use std::future::Future;
use std::sync::mpsc;

use crate::app::App;
use crate::platform::{self, MaybeSend};

#[cfg(not(target_arch = "wasm32"))]
pub type Update = Box<dyn FnOnce(&mut App) + Send>;
#[cfg(target_arch = "wasm32")]
pub type Update = Box<dyn FnOnce(&mut App)>;

/// Wraps a closure as an `Update`.
pub fn update<F>(f: F) -> Update
where
    F: FnOnce(&mut App) + MaybeSend + 'static,
{
    Box::new(f)
}

#[derive(Clone)]
pub struct Tasks {
    sender: mpsc::Sender<Update>,
    ctx: egui::Context,
}

impl Tasks {
    pub fn new(ctx: egui::Context) -> (Self, mpsc::Receiver<Update>) {
        let (sender, receiver) = mpsc::channel();
        (Self { sender, ctx }, receiver)
    }

    /// Runs `future` in the background and applies its resulting update on the UI thread.
    pub fn spawn<F>(&self, future: F)
    where
        F: Future<Output = Update> + MaybeSend + 'static,
    {
        let sender = self.sender.clone();
        let ctx = self.ctx.clone();
        platform::spawn(async move {
            let update = future.await;
            let _ = sender.send(update);
            ctx.request_repaint();
        });
    }

    /// Queues an update for the next frame from any thread or task.
    pub fn send(&self, update: Update) {
        let _ = self.sender.send(update);
        self.ctx.request_repaint();
    }

    pub fn ctx(&self) -> &egui::Context {
        &self.ctx
    }
}
