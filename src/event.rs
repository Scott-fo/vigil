use color_eyre::eyre::OptionExt;
use crossterm::event::Event as CrosstermEvent;
use futures::{FutureExt, StreamExt};
use std::time::Duration;
use tokio::sync::mpsc;

const TICK_FPS: f64 = 30.0;

#[derive(Clone, Debug)]
pub enum Event {
    Tick,
    Crossterm(CrosstermEvent),
}

#[derive(Debug)]
pub struct EventHandler {
    receiver: mpsc::UnboundedReceiver<Event>,
}

impl EventHandler {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let actor = EventTask::new(sender.clone());
        tokio::spawn(async move {
            let _ = actor.run().await;
        });
        Self { receiver }
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
        let tick_rate = Duration::from_secs_f64(1.0 / TICK_FPS);
        let mut reader = crossterm::event::EventStream::new();
        let mut tick = tokio::time::interval(tick_rate);

        loop {
            let tick_delay = tick.tick();
            let crossterm_event = reader.next().fuse();

            tokio::select! {
                _ = tick_delay => {
                    if !self.send(Event::Tick) {
                        break;
                    }
                }
                Some(Ok(event)) = crossterm_event => {
                    if !self.send(Event::Crossterm(event)) {
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    fn send(&self, event: Event) -> bool {
        self.sender.send(event).is_ok()
    }
}
