//! Console output with a live, in-place progress area at the bottom.
//!
//! Regular output lines are queued while the progress display is active and flushed above the
//! progress block on the next tick, so both can coexist without garbling each other.

use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const TICK_PERIOD: Duration = Duration::from_millis(100);

/// Line-oriented builder for one progress frame.
#[derive(Default)]
pub struct ProgressFrame {
    pub display_width: usize,
    pub console_width: usize,
    pub finished: bool,
    lines: Vec<String>,
    current: String,
    pub special_jitter_event: bool,
}

impl ProgressFrame {
    pub fn new(console_width: usize, finished: bool) -> Self {
        Self { display_width: console_width.min(120), console_width, finished, ..Default::default() }
    }
    pub fn append(&mut self, s: &str) -> &mut Self {
        self.current.push_str(s);
        self
    }
    pub fn append_char(&mut self, c: char, count: usize) -> &mut Self {
        for _ in 0..count {
            self.current.push(c);
        }
        self
    }
    pub fn append_fixed(&mut self, s: &str, len: usize) -> &mut Self {
        let n = s.chars().count();
        if n < len {
            self.current.push_str(s);
            self.append_char(' ', len - n);
        } else {
            self.current.extend(s.chars().take(len));
        }
        self
    }
    pub fn append_pad_left(&mut self, v: i64, len: usize) -> &mut Self {
        self.current.push_str(&format!("{:>width$}", v, width = len));
        self
    }
    pub fn append_bar(&mut self, bar_size: usize, fill: f64) -> &mut Self {
        let inner = bar_size.saturating_sub(2);
        let filled = ((inner as f64) * fill.clamp(0.0, 1.0)).ceil() as usize;
        self.current.push('[');
        self.append_char('#', filled.min(inner));
        self.append_char(' ', inner - filled.min(inner));
        self.current.push(']');
        self
    }
    pub fn append_line(&mut self) -> &mut Self {
        let line = std::mem::take(&mut self.current);
        self.lines.push(line);
        self
    }
    pub fn line_count(&self) -> usize {
        self.lines.len() + usize::from(!self.current.is_empty())
    }
    pub fn lines(&self) -> Vec<String> {
        let mut v = self.lines.clone();
        if !self.current.is_empty() {
            v.push(self.current.clone());
        }
        v
    }
}

/// Callback producing the progress frame contents.
pub type ProgressRenderer = dyn FnMut(&mut ProgressFrame) + Send;

struct Shared {
    queued: Mutex<Vec<String>>,
    showing: AtomicBool,
    paused: AtomicBool,
    stop: AtomicBool,
    wake: Condvar,
    wake_lock: Mutex<()>,
    renderer: Mutex<Option<Box<ProgressRenderer>>>,
    /// Number of progress lines currently on screen.
    on_screen: Mutex<usize>,
    show_jitter: AtomicBool,
    is_tty: bool,
}

#[derive(Clone)]
pub struct Console {
    shared: Arc<Shared>,
    thread: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl Default for Console {
    fn default() -> Self {
        Self::new()
    }
}

impl Console {
    pub fn new() -> Self {
        let is_tty = std::io::stdout().is_terminal();
        Self {
            shared: Arc::new(Shared {
                queued: Mutex::new(Vec::new()),
                showing: AtomicBool::new(false),
                paused: AtomicBool::new(false),
                stop: AtomicBool::new(false),
                wake: Condvar::new(),
                wake_lock: Mutex::new(()),
                renderer: Mutex::new(None),
                on_screen: Mutex::new(0),
                show_jitter: AtomicBool::new(false),
                is_tty,
            }),
            thread: Arc::new(Mutex::new(None)),
        }
    }

    pub fn is_terminal(&self) -> bool {
        self.shared.is_tty
    }

    pub fn width() -> usize {
        crossterm::terminal::size().map(|(w, _)| w as usize).ok().filter(|w| *w > 0).unwrap_or(80).max(20)
    }

    pub fn set_show_display_jitter(&self, v: bool) {
        self.shared.show_jitter.store(v, Ordering::Relaxed);
    }

    pub fn set_renderer(&self, renderer: Box<ProgressRenderer>) {
        *self.shared.renderer.lock().unwrap_or_else(|e| e.into_inner()) = Some(renderer);
    }

    pub fn showing_progress(&self) -> bool {
        self.shared.showing.load(Ordering::Relaxed)
    }

    /// Print a line (queued while the progress display is active).
    pub fn write_line(&self, line: &str) {
        if self.showing_progress() {
            self.shared.queued.lock().unwrap_or_else(|e| e.into_inner()).push(line.to_string());
        } else {
            let mut out = std::io::stdout().lock();
            let _ = writeln!(out, "{line}");
            let _ = out.flush();
        }
    }

    pub fn write_lines(&self, lines: &[String]) {
        if lines.is_empty() {
            return;
        }
        if self.showing_progress() {
            self.shared.queued.lock().unwrap_or_else(|e| e.into_inner()).extend(lines.iter().cloned());
        } else {
            let mut out = std::io::stdout().lock();
            let _ = writeln!(out, "{}", lines.join("\n"));
            let _ = out.flush();
        }
    }

    pub fn start_progress_display(&self) {
        if !self.shared.is_tty || self.showing_progress() {
            return;
        }
        self.shared.stop.store(false, Ordering::SeqCst);
        self.shared.showing.store(true, Ordering::SeqCst);
        let shared = Arc::clone(&self.shared);
        let handle = std::thread::Builder::new()
            .name("avd3-progress".into())
            .spawn(move || Self::ticker(shared))
            .expect("spawn progress thread");
        *self.thread.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle);
        let _ = write!(std::io::stdout(), "\x1b[?25l");
    }

    fn ticker(shared: Arc<Shared>) {
        let mut jitter_count: u64 = 0;
        let mut skip_count: u64 = 0;
        // Initial delay like the original (500ms) so the first frame has data.
        std::thread::sleep(Duration::from_millis(500));
        loop {
            if shared.stop.load(Ordering::SeqCst) {
                break;
            }
            if shared.paused.load(Ordering::SeqCst) {
                skip_count += 1;
            } else {
                let started = Instant::now();
                Self::tick(&shared, jitter_count, skip_count, started);
                jitter_count += 1;
            }
            let guard = shared.wake_lock.lock().unwrap_or_else(|e| e.into_inner());
            let _ = shared.wake.wait_timeout(guard, TICK_PERIOD);
        }
    }

    fn tick(shared: &Shared, jitter_count: u64, skip_count: u64, started: Instant) {
        let width = Self::width();
        let queued: Vec<String> = std::mem::take(&mut *shared.queued.lock().unwrap_or_else(|e| e.into_inner()));
        let mut frame = ProgressFrame::new(width, false);
        if let Some(r) = shared.renderer.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            r(&mut frame);
        }
        if shared.show_jitter.load(Ordering::Relaxed) {
            frame.append_line();
            let ms = started.elapsed().as_millis();
            frame.append(&format!("{:04} {:03} {:06} {}", jitter_count, skip_count, ms, if frame.special_jitter_event { format!("{:06}", ms) } else { String::new() }));
        }
        let lines = frame.lines();
        let mut on_screen = shared.on_screen.lock().unwrap_or_else(|e| e.into_inner());
        let mut out = std::io::stdout().lock();
        let mut buf = String::new();
        if *on_screen > 0 {
            buf.push_str(&format!("\x1b[{}A", *on_screen));
        }
        buf.push_str("\r\x1b[J");
        for q in &queued {
            buf.push_str(q);
            buf.push('\n');
        }
        for (i, l) in lines.iter().enumerate() {
            let truncated: String = l.chars().take(width.saturating_sub(1)).collect();
            buf.push_str(&truncated);
            if i + 1 < lines.len() {
                buf.push('\n');
            }
        }
        // Keep the cursor at the end of the last progress line; next tick moves up.
        let _ = out.write_all(buf.as_bytes());
        let _ = out.flush();
        *on_screen = lines.len().saturating_sub(1);
    }

    /// Stop the live display, flushing queued lines and erasing the progress block.
    pub fn stop_progress_display(&self) {
        if !self.showing_progress() {
            return;
        }
        // Let the bars fill up completely before removing them.
        std::thread::sleep(Duration::from_millis(500));
        self.shared.stop.store(true, Ordering::SeqCst);
        {
            let _g = self.shared.wake_lock.lock().unwrap_or_else(|e| e.into_inner());
            self.shared.wake.notify_all();
        }
        if let Some(h) = self.thread.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = h.join();
        }
        self.shared.showing.store(false, Ordering::SeqCst);
        let queued: Vec<String> = std::mem::take(&mut *self.shared.queued.lock().unwrap_or_else(|e| e.into_inner()));
        let mut on_screen = self.shared.on_screen.lock().unwrap_or_else(|e| e.into_inner());
        let mut out = std::io::stdout().lock();
        let mut buf = String::new();
        if *on_screen > 0 {
            buf.push_str(&format!("\x1b[{}A", *on_screen));
        }
        buf.push_str("\r\x1b[J");
        for q in &queued {
            buf.push_str(q);
            buf.push('\n');
        }
        buf.push_str("\x1b[?25h");
        let _ = out.write_all(buf.as_bytes());
        let _ = out.flush();
        *on_screen = 0;
    }

    /// Print one static progress frame (used after processing finished).
    pub fn write_display_progress(&self) {
        let width = Self::width();
        let mut frame = ProgressFrame::new(width, true);
        if let Some(r) = self.shared.renderer.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            r(&mut frame);
        }
        let lines = frame.lines();
        if lines.is_empty() {
            return;
        }
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{}", lines.join("\n"));
        let _ = out.flush();
    }

    /// Pause the live display for interactive input; resumes when the guard is dropped.
    pub fn lock_console(&self) -> ConsoleLock {
        self.shared.paused.store(true, Ordering::SeqCst);
        if self.showing_progress() {
            // Give a running tick time to finish, then erase the progress block.
            std::thread::sleep(TICK_PERIOD);
            let mut on_screen = self.shared.on_screen.lock().unwrap_or_else(|e| e.into_inner());
            let mut out = std::io::stdout().lock();
            let mut buf = String::new();
            if *on_screen > 0 {
                buf.push_str(&format!("\x1b[{}A", *on_screen));
            }
            buf.push_str("\r\x1b[J\x1b[?25h");
            let _ = out.write_all(buf.as_bytes());
            let _ = out.flush();
            *on_screen = 0;
        }
        ConsoleLock { console: self.clone() }
    }
}

pub struct ConsoleLock {
    console: Console,
}

impl Drop for ConsoleLock {
    fn drop(&mut self) {
        if self.console.showing_progress() {
            let _ = write!(std::io::stdout(), "\x1b[?25l");
        }
        self.console.shared.paused.store(false, Ordering::SeqCst);
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        if self.is_tty {
            let _ = write!(std::io::stdout(), "\x1b[?25h");
        }
    }
}
