//! Process-wide diagnostics for packaged builds.
//!
//! Windows release builds have no console, so diagnostics must go to a file.
//! Files rotate daily or when they reach the byte limit and are capped to seven
//! entries. The non-blocking guard is held by Tauri managed state for the
//! process lifetime so shutdown flushes pending records. A panic is written to
//! the same file at once, past that writer ([`install_panic_hook`]).

use std::any::Any;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, TryLockError};
use std::time::{Duration, Instant};

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const MAX_LOG_FILES: usize = 7;
const MAX_LOG_FILE_BYTES: u64 = 10 * 1024 * 1024;
const LOG_FILE_PREFIX: &str = "faforever-client.";
const LOG_FILE_SUFFIX: &str = ".log";

/// How long the panic hook waits for the log file while the writer's thread
/// is in the middle of a line.
const PANIC_WRITE_PATIENCE: Duration = Duration::from_millis(200);

pub struct DiagnosticsGuard {
    _writer: WorkerGuard,
}

pub fn init(log_dir: &Path) -> Result<DiagnosticsGuard, String> {
    let appender = SharedAppender::new(build_appender(log_dir)?);
    let (writer, guard) = tracing_appender::non_blocking(appender.clone());
    let filter = EnvFilter::try_from_env("FAF_LOG")
        .unwrap_or_else(|_| EnvFilter::new("faf_app=info,faforever_rust_client_lib=info,warn"));
    let file = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_writer(writer);

    #[cfg(debug_assertions)]
    tracing_subscriber::registry()
        .with(filter)
        .with(file)
        .with(tracing_subscriber::fmt::layer().compact())
        .try_init()
        .map_err(|error| format!("could not install the diagnostics subscriber: {error}"))?;

    #[cfg(not(debug_assertions))]
    tracing_subscriber::registry()
        .with(filter)
        .with(file)
        .try_init()
        .map_err(|error| format!("could not install the diagnostics subscriber: {error}"))?;

    install_panic_hook(appender);
    tracing::info!(
        retained_files = MAX_LOG_FILES,
        max_file_bytes = MAX_LOG_FILE_BYTES,
        "client diagnostics initialized"
    );
    Ok(DiagnosticsGuard { _writer: guard })
}

/// The rolling file, shared by the non-blocking writer's thread and the panic
/// hook.
#[derive(Clone)]
struct SharedAppender(Arc<Mutex<SizeAndDateRollingAppender>>);

impl SharedAppender {
    fn new(appender: SizeAndDateRollingAppender) -> Self {
        Self(Arc::new(Mutex::new(appender)))
    }

    fn lock(&self) -> MutexGuard<'_, SizeAndDateRollingAppender> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Write `record` now, waiting at most [`PANIC_WRITE_PATIENCE`] for the
    /// writer's thread to finish its line. Gives up rather than waits on a
    /// lock this very thread may hold: a panic inside a write.
    fn write_now(&self, record: &str) {
        let deadline = Instant::now() + PANIC_WRITE_PATIENCE;
        let mut file = loop {
            match self.0.try_lock() {
                Ok(file) => break file,
                Err(TryLockError::Poisoned(poisoned)) => break poisoned.into_inner(),
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(TryLockError::WouldBlock) => return,
            }
        };
        // Nowhere to report a failure: this is the logger, in a panic.
        let _ = file.write_all(record.as_bytes());
        let _ = file.flush();
    }
}

impl Write for SharedAppender {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.lock().write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.lock().flush()
    }
}

/// Write every panic into the log file, with its message, place and thread,
/// before the default hook prints it.
///
/// Straight into the file rather than through `tracing`: the non-blocking
/// writer hands lines to a thread of its own, and a panic that ends the
/// process (on Android one on a thread the system called into, which aborts)
/// leaves that thread no time to write its queue. A phone's log then ended
/// mid-session with no word of why.
fn install_panic_hook(file: SharedAppender) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        file.write_now(&panic_record(
            time::OffsetDateTime::now_utc(),
            thread.name().unwrap_or("unnamed"),
            info.location(),
            panic_message(info.payload()),
        ));
        previous(info);
    }));
}

/// What a panic said: `panic!`'s text, or a placeholder for a payload that is
/// not text (`std::panic::panic_any`).
fn panic_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("a panic without a message")
}

/// A panic as a line of the log, in the shape the file's other lines have:
/// UTC time, level, target, then what happened.
fn panic_record(
    at: time::OffsetDateTime,
    thread: &str,
    location: Option<&std::panic::Location<'_>>,
    message: &str,
) -> String {
    let place = location.map_or_else(
        || "an unknown place".to_owned(),
        |location| {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        },
    );
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}Z ERROR {}: panic in thread {thread} at {place}: {message}\n",
        at.year(),
        at.month() as u8,
        at.day(),
        at.hour(),
        at.minute(),
        at.second(),
        at.microsecond(),
        module_path!(),
    )
}

fn build_appender(log_dir: &Path) -> Result<SizeAndDateRollingAppender, String> {
    SizeAndDateRollingAppender::new(
        log_dir,
        MAX_LOG_FILE_BYTES,
        MAX_LOG_FILES,
        utc_today as DateClock,
    )
    .map_err(|error| format!("could not open the rolling client log: {error}"))
}

type DateClock = fn() -> time::Date;

struct SizeAndDateRollingAppender<C = DateClock> {
    directory: PathBuf,
    current_date: time::Date,
    current_segment: u32,
    current_path: PathBuf,
    current_size: u64,
    max_file_bytes: u64,
    max_files: usize,
    file: File,
    clock: C,
}

impl<C> SizeAndDateRollingAppender<C>
where
    C: Fn() -> time::Date,
{
    fn new(directory: &Path, max_file_bytes: u64, max_files: usize, clock: C) -> io::Result<Self> {
        if max_file_bytes == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "log file byte limit must be greater than zero",
            ));
        }
        if max_files == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "retained log count must be greater than zero",
            ));
        }

        std::fs::create_dir_all(directory)?;
        let current_date = clock();
        let (current_segment, current_path, file, current_size) =
            Self::open_latest(directory, current_date, max_file_bytes)?;
        let mut appender = Self {
            directory: directory.to_path_buf(),
            current_date,
            current_segment,
            current_path,
            current_size,
            max_file_bytes,
            max_files,
            file,
            clock,
        };
        appender.prune_old_files()?;
        Ok(appender)
    }

    fn open_latest(
        directory: &Path,
        date: time::Date,
        max_file_bytes: u64,
    ) -> io::Result<(u32, PathBuf, File, u64)> {
        let mut segment = existing_segments(directory, date)?
            .into_iter()
            .max()
            .unwrap_or(0);
        let mut path = log_path(directory, date, segment);
        let existing_size = path.metadata().map(|metadata| metadata.len()).unwrap_or(0);
        if existing_size >= max_file_bytes {
            segment = segment.saturating_add(1);
            path = log_path(directory, date, segment);
        }
        let file = open_log_file(&path)?;
        let size = file.metadata()?.len();
        Ok((segment, path, file, size))
    }

    fn rotate_if_needed(&mut self) -> io::Result<()> {
        let today = (self.clock)();
        if today != self.current_date {
            let (segment, path, file, size) =
                Self::open_latest(&self.directory, today, self.max_file_bytes)?;
            self.current_date = today;
            self.current_segment = segment;
            self.current_path = path;
            self.file = file;
            self.current_size = size;
            self.prune_old_files()?;
        } else if self.current_size >= self.max_file_bytes {
            self.current_segment = self.current_segment.saturating_add(1);
            self.current_path = log_path(&self.directory, self.current_date, self.current_segment);
            self.file = open_log_file(&self.current_path)?;
            self.current_size = self.file.metadata()?.len();
            self.prune_old_files()?;
        }
        Ok(())
    }

    fn prune_old_files(&mut self) -> io::Result<()> {
        let mut files = client_log_files(&self.directory)?;
        files.sort_by(|left, right| {
            let left_is_current = left == &self.current_path;
            let right_is_current = right == &self.current_path;
            left_is_current
                .cmp(&right_is_current)
                .then_with(|| modified_time(left).cmp(&modified_time(right)))
                .then_with(|| left.cmp(right))
        });
        let remove_count = files.len().saturating_sub(self.max_files);
        // Best effort: an old log that cannot be removed (read-only, held open
        // without delete sharing) must not stop the client from starting or
        // from logging. This is the logger, so there is nowhere to report it.
        for path in files.into_iter().take(remove_count) {
            let _ = std::fs::remove_file(path);
        }
        Ok(())
    }
}

impl<C> Write for SizeAndDateRollingAppender<C>
where
    C: Fn() -> time::Date,
{
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.rotate_if_needed()?;
        let remaining = self.max_file_bytes.saturating_sub(self.current_size);
        let write_len = usize::try_from(remaining)
            .unwrap_or(usize::MAX)
            .min(buffer.len());
        let written = self.file.write(&buffer[..write_len])?;
        self.current_size = self.current_size.saturating_add(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn utc_today() -> time::Date {
    time::OffsetDateTime::now_utc().date()
}

fn log_path(directory: &Path, date: time::Date, segment: u32) -> PathBuf {
    let date = format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        date.month() as u8,
        date.day()
    );
    let segment = if segment == 0 {
        String::new()
    } else {
        format!(".{segment}")
    };
    directory.join(format!("{LOG_FILE_PREFIX}{date}{segment}{LOG_FILE_SUFFIX}"))
}

fn open_log_file(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn existing_segments(directory: &Path, date: time::Date) -> io::Result<Vec<u32>> {
    let base_name = log_path(directory, date, 0)
        .file_name()
        .expect("generated log path has a file name")
        .to_string_lossy()
        .into_owned();
    let segment_prefix = base_name.trim_end_matches(LOG_FILE_SUFFIX);
    Ok(client_log_files(directory)?
        .into_iter()
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?;
            if name == base_name {
                Some(0)
            } else {
                name.strip_prefix(segment_prefix)?
                    .strip_prefix('.')?
                    .strip_suffix(LOG_FILE_SUFFIX)?
                    .parse()
                    .ok()
            }
        })
        .collect())
}

fn client_log_files(directory: &Path) -> io::Result<Vec<PathBuf>> {
    Ok(std::fs::read_dir(directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with(LOG_FILE_PREFIX) && name.ends_with(LOG_FILE_SUFFIX)
                })
        })
        .collect())
}

fn modified_time(path: &Path) -> Option<std::time::SystemTime> {
    path.metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    /// Puts the default hook back however the test ends, so no other test
    /// writes its panics into this one's file.
    struct DefaultHookAfter;

    impl Drop for DefaultHookAfter {
        fn drop(&mut self) {
            let _ = std::panic::take_hook();
        }
    }

    #[test]
    fn a_panic_reaches_the_log_file_with_its_message_and_place() {
        let directory = test_directory("panic");
        let file = SharedAppender::new(build_appender(&directory).expect("appender"));
        let hook = DefaultHookAfter;
        install_panic_hook(file);
        let panicked = std::thread::Builder::new()
            .name("probe".into())
            .spawn(|| panic!("the probe's own panic, number {}", 7))
            .unwrap()
            .join();
        drop(hook);
        assert!(panicked.is_err());

        // Not flushed by any writer thread: the hook wrote it itself.
        let files = client_log_files(&directory).unwrap();
        assert_eq!(files.len(), 1);
        let text = std::fs::read_to_string(&files[0]).unwrap();
        let line = text
            .lines()
            .find(|line| line.contains("the probe's own panic"))
            .unwrap_or_else(|| panic!("no panic line in {text:?}"));
        assert!(line.contains(" ERROR "), "{line}");
        assert!(line.contains("panic in thread probe at "), "{line}");
        assert!(line.contains("diagnostics.rs:"), "{line}");
        assert!(
            line.ends_with(": the probe's own panic, number 7"),
            "{line}"
        );
        remove_test_directory(directory);
    }

    #[test]
    fn a_panic_record_reads_like_the_lines_around_it() {
        let at = time::Date::from_calendar_date(2026, time::Month::November, 24)
            .unwrap()
            .with_hms_micro(18, 5, 9, 123_456)
            .unwrap()
            .assume_utc();
        let location = std::panic::Location::caller();
        let record = panic_record(at, "tokio-runtime-worker", Some(location), "oh no");
        assert_eq!(
            record,
            format!(
                "2026-11-24T18:05:09.123456Z ERROR faforever_rust_client_lib::diagnostics: \
                 panic in thread tokio-runtime-worker at {}:{}:{}: oh no\n",
                location.file(),
                location.line(),
                location.column()
            )
        );
        assert!(panic_record(at, "main", None, "x").contains(" at an unknown place: x"));
    }

    #[test]
    fn a_panic_s_message_is_its_text_whatever_the_payload() {
        let text: Box<dyn Any + Send> = Box::new("plain");
        let formatted: Box<dyn Any + Send> = Box::new(String::from("formatted 7"));
        let other: Box<dyn Any + Send> = Box::new(7_u32);
        assert_eq!(panic_message(text.as_ref()), "plain");
        assert_eq!(panic_message(formatted.as_ref()), "formatted 7");
        assert_eq!(panic_message(other.as_ref()), "a panic without a message");
    }

    #[test]
    fn rolling_writer_creates_a_client_log() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_root = std::env::temp_dir();
        let directory = temp_root.join(format!(
            "faforever-client-diagnostics-test-{}-{unique}",
            std::process::id()
        ));
        let mut appender = build_appender(&directory).expect("appender");
        writeln!(appender, "diagnostic probe").expect("write log");
        appender.flush().expect("flush log");
        drop(appender);

        let files = std::fs::read_dir(&directory)
            .expect("log directory")
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(files.len(), 1);
        assert!(files[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("faforever-client."));
        assert_eq!(
            files[0].extension().and_then(|ext| ext.to_str()),
            Some("log")
        );
        assert!(std::fs::read_to_string(&files[0])
            .expect("read log")
            .contains("diagnostic probe"));

        assert!(directory.starts_with(&temp_root));
        std::fs::remove_dir_all(directory).expect("remove isolated test directory");
    }

    #[test]
    fn rolling_writer_rotates_before_exceeding_the_byte_limit() {
        let directory = test_directory("size");
        let date = time::Date::from_calendar_date(2026, time::Month::August, 9).unwrap();
        let mut appender =
            SizeAndDateRollingAppender::new(&directory, 12, 7, move || date).expect("appender");

        appender.write_all(b"1234567890abcdefghijklmnop").unwrap();
        appender.flush().unwrap();
        drop(appender);

        let mut files = client_log_files(&directory).unwrap();
        files.sort();
        assert_eq!(files.len(), 3);
        assert!(files
            .iter()
            .all(|path| path.metadata().unwrap().len() <= 12));
        assert_eq!(
            files
                .iter()
                .map(|path| path.metadata().unwrap().len())
                .sum::<u64>(),
            26
        );
        remove_test_directory(directory);
    }

    #[test]
    fn rolling_writer_rotates_when_the_utc_date_changes() {
        let directory = test_directory("date");
        let first = time::Date::from_calendar_date(2026, time::Month::August, 9).unwrap();
        let second = first.next_day().unwrap();
        let clock = Arc::new(Mutex::new(first));
        let read_clock = Arc::clone(&clock);
        let mut appender = SizeAndDateRollingAppender::new(&directory, 100, 7, move || {
            *read_clock.lock().unwrap()
        })
        .expect("appender");

        appender.write_all(b"first day\n").unwrap();
        *clock.lock().unwrap() = second;
        appender.write_all(b"second day\n").unwrap();
        appender.flush().unwrap();
        drop(appender);

        let files = client_log_files(&directory).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files
            .iter()
            .any(|path| path.to_string_lossy().contains("2026-08-09")));
        assert!(files
            .iter()
            .any(|path| path.to_string_lossy().contains("2026-08-10")));
        remove_test_directory(directory);
    }

    #[test]
    fn rolling_writer_retains_only_the_newest_seven_files() {
        let directory = test_directory("retention");
        std::fs::create_dir_all(&directory).unwrap();
        for day in 1..=9 {
            let path = directory.join(format!("faforever-client.2026-07-{day:02}.log"));
            std::fs::write(path, format!("day {day}")).unwrap();
        }
        let date = time::Date::from_calendar_date(2026, time::Month::August, 9).unwrap();
        let appender =
            SizeAndDateRollingAppender::new(&directory, 100, 7, move || date).expect("appender");
        let current = appender.current_path.clone();
        drop(appender);

        let files = client_log_files(&directory).unwrap();
        assert_eq!(files.len(), 7);
        assert!(files.contains(&current));
        remove_test_directory(directory);
    }

    fn test_directory(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "faforever-client-diagnostics-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn remove_test_directory(directory: PathBuf) {
        assert!(directory.starts_with(std::env::temp_dir()));
        std::fs::remove_dir_all(directory).expect("remove isolated test directory");
    }
}
