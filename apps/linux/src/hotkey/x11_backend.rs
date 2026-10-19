//! Serializes blocking X protocol control requests away from the GTK thread.

use std::sync::Arc;

use super::Failure;
use crate::x11::{Modifiers, X11};

pub(super) struct Outcome {
    pub result: Result<(), Failure>,
    pub grabbed: bool,
}

enum Action {
    Register { keysym: u32, modifiers: Modifiers, keep: bool },
    Ungrab,
}

enum Command {
    Apply(Action, Option<async_channel::Sender<Outcome>>),
    Stop,
}

pub(super) struct Worker {
    commands: async_channel::Sender<Command>,
    done: async_channel::Receiver<()>,
}

impl Worker {
    pub fn new(connection: Arc<X11>) -> Option<Self> {
        Self::spawn(move |action| {
            let result = match action {
                Action::Register { keysym, modifiers, keep } => {
                    let result = connection.grab(keysym, modifiers);
                    if !keep {
                        connection.ungrab();
                    }
                    result
                }
                Action::Ungrab => {
                    connection.ungrab();
                    Ok(())
                }
            };
            Outcome { result, grabbed: connection.is_grabbed() }
        })
    }

    fn spawn(mut apply: impl FnMut(Action) -> Outcome + Send + 'static) -> Option<Self> {
        let (commands, requests) = async_channel::unbounded();
        let (finished, done) = async_channel::bounded(1);
        std::thread::Builder::new()
            .name("qantrip-hotkey-control".into())
            .spawn(move || {
                while let Ok(command) = requests.recv_blocking() {
                    match command {
                        Command::Apply(action, reply) => {
                            let result = apply(action);
                            if let Some(reply) = reply {
                                let _ = reply.try_send(result);
                            }
                        }
                        Command::Stop => break,
                    }
                }
                apply(Action::Ungrab);
                let _ = finished.try_send(());
            })
            .ok()?;
        Some(Self { commands, done })
    }

    pub async fn register(&self, keysym: u32, modifiers: Modifiers, keep: bool) -> Outcome {
        let (reply, response) = async_channel::bounded(1);
        let command = Command::Apply(Action::Register { keysym, modifiers, keep }, Some(reply));
        if self.commands.try_send(command).is_err() {
            return Outcome { result: Err(Failure::Disconnected), grabbed: false };
        }
        response.recv().await.unwrap_or(Outcome { result: Err(Failure::Disconnected), grabbed: false })
    }

    pub fn ungrab(&self) {
        let _ = self.commands.try_send(Command::Apply(Action::Ungrab, None));
    }

    pub fn stop(&self) {
        let _ = self.commands.try_send(Command::Stop);
    }

    pub async fn finish(&self) {
        self.stop();
        let _ = self.done.recv().await;
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn queued_registration_pause_and_cleanup_are_serial() {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let observed = operations.clone();
        let worker = Worker::spawn(move |action| {
            let code = match action {
                Action::Register { keysym, .. } => keysym,
                Action::Ungrab => 0,
            };
            observed.lock().unwrap().push(code);
            Outcome { result: if code == 2 { Err(Failure::Conflict) } else { Ok(()) }, grabbed: code != 0 }
        })
        .unwrap();
        let context = gtk::glib::MainContext::new();
        context.block_on(async {
            assert!(worker.register(1, Modifiers::default(), true).await.result.is_ok());
            worker.ungrab();
            assert_eq!(worker.register(2, Modifiers::default(), true).await.result, Err(Failure::Conflict));
            worker.finish().await;
        });
        assert_eq!(*operations.lock().unwrap(), [1, 0, 2, 0]);
    }

    #[test]
    fn slow_server_does_not_block_the_main_context() {
        let main_thread = std::thread::current().id();
        let (release, wait) = async_channel::bounded(1);
        let worker = Worker::spawn(move |action| {
            assert_ne!(std::thread::current().id(), main_thread);
            if matches!(action, Action::Register { .. }) {
                wait.recv_blocking().unwrap();
            }
            Outcome { result: Ok(()), grabbed: true }
        })
        .unwrap();
        let context = gtk::glib::MainContext::new();
        context
            .with_thread_default(|| {
                context.block_on(async {
                    let registering = worker.register(1, Modifiers::default(), true);
                    let ui = async {
                        gtk::glib::timeout_future(std::time::Duration::from_millis(5)).await;
                        release.send(()).await.unwrap();
                    };
                    let (outcome, ()) = futures_util::future::join(registering, ui).await;
                    assert!(outcome.result.is_ok());
                    worker.finish().await;
                })
            })
            .unwrap();
    }
}
