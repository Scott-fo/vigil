use std::{collections::HashSet, path::PathBuf, time::Duration};

use tokio::sync::mpsc;

use crate::event::Event;

const WATCH_DEBOUNCE: Duration = Duration::from_millis(200);

pub(super) async fn run_debounce_loop(
    mut signal_receiver: mpsc::UnboundedReceiver<Vec<PathBuf>>,
    event_sender: mpsc::UnboundedSender<Event>,
) {
    while let Some(initial_paths) = signal_receiver.recv().await {
        let mut changed_paths = collect_paths(initial_paths);
        loop {
            let delay = tokio::time::sleep(WATCH_DEBOUNCE);
            tokio::pin!(delay);

            tokio::select! {
                _ = &mut delay => {
                    let _ = event_sender.send(Event::RepoChanged(changed_paths.into_iter().collect()));
                    break;
                }
                maybe_paths = signal_receiver.recv() => {
                    match maybe_paths {
                        Some(paths) => {
                            changed_paths.extend(collect_paths(paths));
                        }
                        None => {
                            let _ = event_sender.send(Event::RepoChanged(changed_paths.into_iter().collect()));
                            return;
                        }
                    }
                }
            }
        }
    }
}

fn collect_paths(paths: Vec<PathBuf>) -> HashSet<PathBuf> {
    paths.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn debounce_coalesces_paths_before_sending_repo_changed() {
        let (signal_sender, signal_receiver) = mpsc::unbounded_channel();
        let (event_sender, mut event_receiver) = mpsc::unbounded_channel();
        let task = tokio::spawn(run_debounce_loop(signal_receiver, event_sender));

        signal_sender
            .send(vec![PathBuf::from("src/lib.rs")])
            .expect("signal receiver should be open");
        signal_sender
            .send(vec![PathBuf::from("src/main.rs")])
            .expect("signal receiver should be open");

        let event = tokio::time::timeout(Duration::from_secs(1), event_receiver.recv())
            .await
            .expect("debounced event should arrive")
            .expect("event channel should stay open");

        let Event::RepoChanged(mut paths) = event else {
            panic!("expected repo changed event");
        };
        paths.sort();
        assert_eq!(
            paths,
            vec![PathBuf::from("src/lib.rs"), PathBuf::from("src/main.rs")]
        );

        task.abort();
    }
}
