use color_eyre::eyre::OptionExt;
use crossterm::event::Event as CrosstermEvent;
use futures::StreamExt;
use tokio::sync::mpsc;

use crate::git::SharedHighlightRegistry;

#[derive(Clone, Debug)]
pub enum Event {
    Crossterm(CrosstermEvent),
    HighlightRegistryReady(Result<SharedHighlightRegistry, String>),
    RemotePushFinished(Result<String, String>),
    ClearSnackbar(u64),
}

#[derive(Debug)]
pub struct EventHandler {
    sender: mpsc::UnboundedSender<Event>,
    receiver: mpsc::UnboundedReceiver<Event>,
}

impl Default for EventHandler {
    fn default() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let actor = EventTask::new(sender.clone());
        tokio::spawn(async move {
            let _ = actor.run().await;
        });
        Self { sender, receiver }
    }
}

impl EventHandler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sender(&self) -> mpsc::UnboundedSender<Event> {
        self.sender.clone()
    }

    pub async fn next(&mut self) -> color_eyre::Result<Event> {
        self.receiver
            .recv()
            .await
            .ok_or_eyre("failed to receive event")
    }
}

struct EventTask {
    sender: mpsc::UnboundedSender<Event>,
}

impl EventTask {
    fn new(sender: mpsc::UnboundedSender<Event>) -> Self {
        Self { sender }
    }

    async fn run(self) -> color_eyre::Result<()> {
        let mut reader = crossterm::event::EventStream::new();

        while let Some(result) = reader.next().await {
            if let Ok(event) = result {
                if !self.send(Event::Crossterm(event)) {
                    break;
                }
            }
        }

        Ok(())
    }

    fn send(&self, event: Event) -> bool {
        self.sender.send(event).is_ok()
    }
}
