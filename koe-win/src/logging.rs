use log::{Level, LevelFilter, Log, Metadata, Record};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

static LOGGER: FileLogger = FileLogger;
static LOG_FILE: OnceLock<Mutex<File>> = OnceLock::new();

struct FileLogger;

fn timestamp() -> String {
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let s = secs % 60;
    let ms = dur.subsec_millis();
    format!("{h:02}:{m:02}:{s:02}.{ms:03}")
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Debug
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        if let Some(file) = LOG_FILE.get() {
            if let Ok(mut f) = file.lock() {
                let _ = writeln!(f, "[{}] {} {}", timestamp(), record.level(), record.args());
                let _ = f.flush();
            }
        }
    }

    fn flush(&self) {
        if let Some(file) = LOG_FILE.get() {
            if let Ok(mut f) = file.lock() {
                let _ = f.flush();
            }
        }
    }
}

pub fn init() {
    let dir = match std::env::var("LOCALAPPDATA") {
        Ok(base) => std::path::PathBuf::from(base).join("koe"),
        Err(_) => std::env::current_dir().unwrap_or_default(),
    };
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("koe.log");

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path);

    match file {
        Ok(f) => {
            let _ = LOG_FILE.set(Mutex::new(f));
        }
        Err(_) => {
            env_logger::init();
            return;
        }
    }

    let _ = log::set_logger(&LOGGER);
    log::set_max_level(LevelFilter::Debug);
}
