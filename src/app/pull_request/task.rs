use std::time::Duration;

use tokio::{sync::mpsc, task::JoinHandle};

use crate::event::Event;

/// A background task that is aborted when dropped, so dropping the state that
/// owns it cancels it. Aborting a task running `gh` kills the process.
#[derive(Debug)]
pub(in crate::app) struct OwnedTask(JoinHandle<()>);

impl OwnedTask {
    pub(super) fn new(handle: JoinHandle<()>) -> Self {
        Self(handle)
    }
}

impl Drop for OwnedTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The latest request of one kind. Beginning a request supersedes the
/// previous one: its task is aborted and a response that still arrives is
/// recognised as stale by its id and dropped.
#[derive(Debug, Default)]
pub(super) struct RequestSlot {
    id: u64,
    in_flight: bool,
    task: Option<OwnedTask>,
}

impl RequestSlot {
    pub(super) fn begin(&mut self) -> u64 {
        self.id = self.id.wrapping_add(1);
        self.in_flight = true;
        self.task = None;
        self.id
    }

    pub(super) fn attach(&mut self, id: u64, handle: JoinHandle<()>) {
        if id == self.id && self.in_flight {
            self.task = Some(OwnedTask::new(handle));
        } else {
            handle.abort();
        }
    }

    /// Accepts the response to request `id` if it is still the latest one.
    pub(super) fn complete(&mut self, id: u64) -> bool {
        if id != self.id || !self.in_flight {
            return false;
        }
        self.in_flight = false;
        self.task = None;
        true
    }

    pub(super) fn cancel(&mut self) {
        self.id = self.id.wrapping_add(1);
        self.in_flight = false;
        self.task = None;
    }

    pub(super) fn in_flight(&self) -> bool {
        self.in_flight
    }

    /// Whether request `id` is the latest one and still running.
    pub(super) fn is_current(&self, id: u64) -> bool {
        self.in_flight && id == self.id
    }

    /// Like [`Self::attach`], but the task keeps running if the request is
    /// superseded or the slot is dropped. For writes, which must not stop
    /// midway.
    pub(super) fn attach_detached(&mut self, id: u64, handle: JoinHandle<()>) {
        if id == self.id && self.in_flight {
            self.task = None;
        }
        drop(handle);
    }

    #[cfg(test)]
    pub(super) fn current_id(&self) -> u64 {
        self.id
    }
}

/// Sends `event()` every `period`, starting one period from now, until
/// dropped.
pub(super) fn spawn_ticker(
    sender: mpsc::UnboundedSender<Event>,
    period: Duration,
    event: fn() -> Event,
) -> OwnedTask {
    OwnedTask::new(tokio::spawn(async move {
        let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            if sender.send(event()).is_err() {
                break;
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::RequestSlot;

    #[test]
    fn only_the_latest_request_completes() {
        let mut slot = RequestSlot::default();
        let first = slot.begin();
        let second = slot.begin();

        assert!(!slot.complete(first));
        assert!(slot.in_flight());
        assert!(slot.complete(second));
        assert!(!slot.complete(second), "a response is accepted once");
        assert!(!slot.in_flight());
    }

    #[test]
    fn cancelled_requests_never_complete() {
        let mut slot = RequestSlot::default();
        let id = slot.begin();
        slot.cancel();

        assert!(!slot.complete(id));
    }
}
