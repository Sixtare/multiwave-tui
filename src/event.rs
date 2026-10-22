use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use color_eyre::Result;
use ratatui::crossterm::event::{
    self, Event as CrosstermEvent, KeyEvent, KeyEventKind, MouseEvent,
};

/// Terminal events.
#[derive(Clone, Debug)]
pub enum Event {
    /// Terminal tick.
    Tick,
    /// Key press.
    Key(KeyEvent),
    /// Mouse click/scroll.
    Mouse(MouseEvent),
    /// Terminal resize.
    Resize(u16, u16),
    /// Audio visualization bars.
    AudioBars(Vec<f32>),
    /// Audio stream error reported by the audio callback.
    AudioError(String),
    /// The active audio endpoint was disconnected or its stream was invalidated.
    AudioDeviceLost,
}

/// Forwards terminal events used by the app and ignores the rest.
///
/// Windows always delivers focus events, so they must be handled explicitly instead of falling
/// through to `unimplemented!()`. Panicking here would kill the event thread while the audio
/// callback keeps the channel alive, leaving the UI running but unresponsive to keys.
fn forward_terminal_event(
    terminal_event: CrosstermEvent,
    sender: &mpsc::Sender<Event>,
) -> std::result::Result<(), mpsc::SendError<Event>> {
    match terminal_event {
        CrosstermEvent::Key(key_event) if key_event.kind == KeyEventKind::Press => {
            sender.send(Event::Key(key_event))
        }
        // Ignore release/repeat events, especially the release events emitted on Windows.
        CrosstermEvent::Key(_) => Ok(()),
        CrosstermEvent::Mouse(mouse_event) => sender.send(Event::Mouse(mouse_event)),
        CrosstermEvent::Resize(width, height) => sender.send(Event::Resize(width, height)),
        CrosstermEvent::FocusGained | CrosstermEvent::FocusLost => Ok(()),
        // Paste is only emitted when bracketed paste is enabled, but ignore any future variant
        // rather than panicking the terminal event thread.
        _ => Ok(()),
    }
}

/// Terminal event handler.
#[derive(Debug)]
pub struct EventHandler {
    /// Event sender channel.
    #[allow(dead_code)]
    sender: mpsc::Sender<Event>,
    /// Event receiver channel.
    receiver: mpsc::Receiver<Event>,
    /// Event handler thread.
    #[allow(dead_code)]
    handler: thread::JoinHandle<()>,
}

impl EventHandler {
    /// Constructs a new instance of [`EventHandler`].
    pub fn new(tick_rate: u64) -> Self {
        let tick_rate = Duration::from_millis(tick_rate);
        let (sender, receiver) = mpsc::channel();
        let handler = {
            let sender = sender.clone();
            thread::spawn(move || {
                let mut last_tick = Instant::now();
                loop {
                    let timeout = tick_rate
                        .checked_sub(last_tick.elapsed())
                        .unwrap_or(tick_rate);

                    if event::poll(timeout).expect("unable to poll for event") {
                        let terminal_event = event::read().expect("unable to read event");
                        // The receiver is gone once the app exits; stop the thread instead of
                        // panicking during shutdown.
                        if forward_terminal_event(terminal_event, &sender).is_err() {
                            return;
                        }
                    }

                    if last_tick.elapsed() >= tick_rate {
                        if sender.send(Event::Tick).is_err() {
                            return;
                        }
                        last_tick = Instant::now();
                    }
                }
            })
        };
        Self {
            sender,
            receiver,
            handler,
        }
    }

    /// Returns a clone of the event sender.
    pub fn sender(&self) -> mpsc::Sender<Event> {
        self.sender.clone()
    }

    /// Receive the next event from the handler thread.
    ///
    /// This function will always block the current thread if
    /// there is no data available and it's possible for more data to be sent.
    pub fn next(&self) -> Result<Event> {
        Ok(self.receiver.recv()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

    #[test]
    fn test_focus_events_are_ignored() {
        let (sender, receiver) = mpsc::channel();

        forward_terminal_event(CrosstermEvent::FocusGained, &sender).unwrap();
        forward_terminal_event(CrosstermEvent::FocusLost, &sender).unwrap();

        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
    }

    #[test]
    fn test_key_press_is_forwarded() {
        let (sender, receiver) = mpsc::channel();
        let key_event = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);

        forward_terminal_event(CrosstermEvent::Key(key_event), &sender).unwrap();

        match receiver.try_recv().unwrap() {
            Event::Key(forwarded) => {
                assert_eq!(forwarded.code, KeyCode::Char('q'));
                assert_eq!(forwarded.kind, KeyEventKind::Press);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn test_key_release_is_ignored() {
        let (sender, receiver) = mpsc::channel();
        let key_event = KeyEvent::new_with_kind(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );

        forward_terminal_event(CrosstermEvent::Key(key_event), &sender).unwrap();

        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
    }
}
