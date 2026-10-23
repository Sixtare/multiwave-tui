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

/// Events drained from the channel in one main loop iteration.
///
/// The main loop renders once per batch instead of once per event. Rendering per event lets the
/// queue grow without bound whenever a draw is slower than the combined tick and audio rate
/// (common on Windows consoles), so input and bars lag further behind the longer the app runs.
#[derive(Debug, Default)]
pub struct EventBatch {
    /// Number of ticks received; each one advances the animation by one step.
    pub ticks: u32,
    /// Most recent audio bars; older frames in the same batch are superseded.
    pub latest_bars: Option<Vec<f32>>,
    /// Terminal width after the last resize in the batch.
    pub latest_width: Option<u16>,
    /// Events that must be handled individually, in arrival order.
    pub events: Vec<BatchEvent>,
}

/// Events that keep their identity and order inside an [`EventBatch`].
#[derive(Clone, Debug)]
pub enum BatchEvent {
    /// Key press.
    Key(KeyEvent),
    /// Audio stream error reported by the audio callback.
    AudioError(String),
    /// The active audio endpoint was disconnected or its stream was invalidated.
    AudioDeviceLost,
}

impl EventBatch {
    /// Adds an event to the batch, collapsing the ones that only matter as the latest value.
    pub fn push(&mut self, event: Event) {
        match event {
            Event::Tick => self.ticks += 1,
            Event::AudioBars(bars) => self.latest_bars = Some(bars),
            Event::Resize(width, _) => self.latest_width = Some(width),
            // Mouse input is unused.
            Event::Mouse(_) => {}
            Event::Key(key_event) => self.events.push(BatchEvent::Key(key_event)),
            Event::AudioError(message) => self.events.push(BatchEvent::AudioError(message)),
            Event::AudioDeviceLost => self.events.push(BatchEvent::AudioDeviceLost),
        }
    }

    /// Returns true when the batch contains a tick; rendering and device polling follow ticks.
    pub fn has_tick(&self) -> bool {
        self.ticks > 0
    }
}

/// Blocks for the next event, then drains everything already queued into a single batch.
fn next_batch(receiver: &mpsc::Receiver<Event>) -> Result<EventBatch> {
    let mut batch = EventBatch::default();
    batch.push(receiver.recv()?);
    while let Ok(event) = receiver.try_recv() {
        batch.push(event);
    }
    Ok(batch)
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

    /// Receive every pending event from the handler thread as one batch.
    ///
    /// This function will always block the current thread if
    /// there is no data available and it's possible for more data to be sent.
    pub fn next_batch(&self) -> Result<EventBatch> {
        next_batch(&self.receiver)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyModifiers, MouseEventKind};

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
    fn test_batch_drains_backlog_keeping_keys_and_latest_bars() {
        let (sender, receiver) = mpsc::channel();
        for level in 0..50 {
            sender.send(Event::AudioBars(vec![level as f32])).unwrap();
        }
        sender
            .send(Event::Key(KeyEvent::new(
                KeyCode::Char('s'),
                KeyModifiers::NONE,
            )))
            .unwrap();
        sender.send(Event::Tick).unwrap();
        sender
            .send(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))
            .unwrap();
        sender.send(Event::Tick).unwrap();

        let batch = next_batch(&receiver).unwrap();

        assert!(batch.has_tick());
        assert_eq!(batch.ticks, 2);
        assert_eq!(batch.latest_bars, Some(vec![49.0]));
        let keys: Vec<KeyCode> = batch
            .events
            .iter()
            .map(|event| match event {
                BatchEvent::Key(key) => key.code,
                other => panic!("unexpected event: {other:?}"),
            })
            .collect();
        assert_eq!(keys, vec![KeyCode::Char('s'), KeyCode::Down]);
        // The whole backlog is drained into this one batch.
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
    }

    #[test]
    fn test_audio_and_mouse_alone_have_no_tick() {
        let (sender, receiver) = mpsc::channel();
        sender.send(Event::AudioBars(vec![0.5])).unwrap();
        sender
            .send(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Moved,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::NONE,
            }))
            .unwrap();
        sender.send(Event::Resize(80, 24)).unwrap();

        let batch = next_batch(&receiver).unwrap();

        assert!(!batch.has_tick());
        assert_eq!(batch.latest_bars, Some(vec![0.5]));
        assert_eq!(batch.latest_width, Some(80));
        assert!(batch.events.is_empty());
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
