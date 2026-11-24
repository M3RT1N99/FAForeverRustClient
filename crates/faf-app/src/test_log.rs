//! The log lines a test can read: each event's level, message and fields, for
//! the lines that carry a decision (a code, a count, a version) a phone's log
//! is read for.
//!
//! Kept per thread ([`capture`]), so a test on `#[tokio::test]`'s
//! single-threaded runtime sees its own lines, the tasks it spawns included,
//! and nothing from the tests beside it.
//!
//! The subscriber is the process's global one, installed by the first
//! capture, rather than a thread's own (`set_default`). tracing caches each
//! call site's interest, and while one thread-local subscriber is the only one
//! registered, a call site another test reaches first is cached from that
//! test's thread, which has none: as disabled, for every thread. A line went
//! missing whenever a test beside the capturing one ran the same code.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::sync::{Arc, Mutex, Once};

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Metadata, Subscriber};

/// One event, as it was logged.
#[derive(Debug, Clone)]
pub(crate) struct LogLine {
    pub(crate) level: Level,
    pub(crate) message: String,
    /// Each field but the message, as `Display` (`%`) or `Debug` (`?`) wrote it.
    pub(crate) fields: BTreeMap<&'static str, String>,
}

impl LogLine {
    pub(crate) fn field(&self, name: &str) -> Option<&str> {
        self.fields.get(name).map(String::as_str)
    }
}

/// The lines logged on this thread since [`capture`].
#[derive(Clone, Default)]
pub(crate) struct Logs(Arc<Mutex<Vec<LogLine>>>);

impl Logs {
    /// The one line whose message is `message`. Panics on none or several,
    /// because a test that pins a line means exactly one.
    pub(crate) fn line(&self, message: &str) -> LogLine {
        let lines = self.0.lock().unwrap();
        let found: Vec<&LogLine> = lines
            .iter()
            .filter(|line| line.message == message)
            .collect();
        match found.as_slice() {
            [line] => (*line).clone(),
            _ => panic!(
                "expected one line {message:?}, found {}; logged: {:#?}",
                found.len(),
                *lines
            ),
        }
    }
}

thread_local! {
    static CAPTURING: RefCell<Option<Logs>> = const { RefCell::new(None) };
}

/// Stops the capture on its thread when dropped.
pub(crate) struct Capturing(());

impl Drop for Capturing {
    fn drop(&mut self) {
        CAPTURING.with(|capturing| capturing.borrow_mut().take());
    }
}

/// Keep every line logged on this thread until the guard is dropped.
pub(crate) fn capture() -> (Logs, Capturing) {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        // Another global one would only mean no capture works: say so.
        tracing::subscriber::set_global_default(Router)
            .expect("no other global subscriber in faf-app's tests");
    });
    let logs = Logs::default();
    CAPTURING.with(|capturing| {
        let previous = capturing.borrow_mut().replace(logs.clone());
        assert!(previous.is_none(), "one capture per thread at a time");
    });
    (logs, Capturing(()))
}

/// Hands each event to the capture of the thread it happens on, if any.
struct Router;

impl Subscriber for Router {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        CAPTURING.with(|capturing| {
            let Some(logs) = capturing.borrow().clone() else {
                return;
            };
            let mut fields = Fields::default();
            event.record(&mut fields);
            logs.0.lock().unwrap().push(LogLine {
                level: *event.metadata().level(),
                message: fields.message,
                fields: fields.values,
            });
        });
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

#[derive(Default)]
struct Fields {
    message: String,
    values: BTreeMap<&'static str, String>,
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.keep(field, value.to_owned());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        self.keep(field, format!("{value:?}"));
    }
}

impl Fields {
    fn keep(&mut self, field: &Field, value: String) {
        if field.name() == "message" {
            self.message = value;
        } else {
            self.values.insert(field.name(), value);
        }
    }
}
