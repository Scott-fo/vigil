use futures::StreamExt;
use tokio::{sync::mpsc, task::JoinHandle};

use super::Event;

#[derive(Debug)]
struct EventTask {
    sender: mpsc::UnboundedSender<Event>,
}

pub(super) fn spawn_event_task(sender: mpsc::UnboundedSender<Event>) -> JoinHandle<()> {
    let actor = EventTask::new(sender);
    tokio::spawn(async move {
        let _ = actor.run().await;
    })
}

impl EventTask {
    fn new(sender: mpsc::UnboundedSender<Event>) -> Self {
        Self { sender }
    }

    async fn run(self) -> color_eyre::Result<()> {
        let mut reader = crossterm::event::EventStream::new();

        while let Some(result) = reader.next().await {
            if let Ok(event) = result
                && !self.send(Event::Crossterm(event))
            {
                break;
            }
        }

        Ok(())
    }

    fn send(&self, event: Event) -> bool {
        self.sender.send(event).is_ok()
    }
}
