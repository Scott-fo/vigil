use color_eyre::eyre::OptionExt;
use tokio::{sync::mpsc, task::JoinHandle};

use super::{Event, task::spawn_event_task};

#[derive(Debug)]
pub struct EventHandler {
    sender: mpsc::UnboundedSender<Event>,
    receiver: mpsc::UnboundedReceiver<Event>,
    task: Option<JoinHandle<()>>,
}

impl Default for EventHandler {
    fn default() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let task = Some(spawn_event_task(sender.clone()));
        Self {
            sender,
            receiver,
            task,
        }
    }
}

impl EventHandler {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn without_event_task() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self {
            sender,
            receiver,
            task: None,
        }
    }

    pub fn sender(&self) -> mpsc::UnboundedSender<Event> {
        self.sender.clone()
    }

    pub fn suspend(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }

    pub fn resume(&mut self) {
        if self.task.is_none() {
            self.task = Some(spawn_event_task(self.sender.clone()));
        }
    }

    pub async fn next(&mut self) -> color_eyre::Result<Event> {
        self.receiver
            .recv()
            .await
            .ok_or_eyre("failed to receive event")
    }
}
