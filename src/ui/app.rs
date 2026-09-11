//! The command line application: configuration checks, processing and per-file handling.

use super::console::Console;
use super::errors::{error_file_stamp, AvdError};
use super::file_move::{expand_placeholders, finalize_destination, FileMoveScript};
use super::progress_display::{DisplayOptions, ProgressDisplay};
use crate::info::meta::container_types;
use crate::info::providers::{default_info_provider_factories, InfoProviderSetup};
use crate::info::FileMetaInfo;
use crate::misc::append_line::AppendLineManager;
use crate::misc::create_directory_chain;
use crate::processing::consumers::{default_block_consumer_factories, BlockConsumer, BlockConsumerFactory};
use crate::processing::progress::BytesReadProgress;
use crate::processing::stream_consumer::{StreamConsumerCollection, StreamConsumerFactory, StreamResult};
use crate::processing::stream_provider::{NullStreamProvider, StreamFromPathsProvider, StreamProvider};
use crate::processing::{CancelToken, MirroredBufferPool, ProcessingError};
use crate::reporting::{default_report_factories, ReportFactory};
use crate::settings::help::{render, Style, VERSION};
use crate::settings::{cli, FileMoveMode, Settings};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Outcome of the configuration phase.
pub enum Startup {
    Continue,
    /// Stop before processing (`reason` is printed when non-empty).
    Cancel { reason: String, exit_code: i32 },
}

pub struct App {
    settings: Settings,
    paths: Vec<String>,
    console: Console,
    consumer_factories: Vec<BlockConsumerFactory>,
    report_factories: Vec<ReportFactory>,
    selected_consumers: Vec<(String, Vec<String>)>,
    file_paths_to_skip: HashSet<String>,
    file_move: Option<Mutex<FileMoveScript>>,
    line_writer: AppendLineManager,
    report_save_lock: Mutex<()>,
}

/// Entry point used by `main`: returns the process exit code.
pub fn run(args: Vec<String>) -> i32 {
    let print_args = args.iter().any(|a| a == "PRINTARGS");
    let (settings, parse) = match cli::parse_settings(&args) {
        Ok(v) => v,
        Err(e) => {
            println!("Error while parsing commandline arguments:");
            println!("{e}");
            return 2;
        }
    };
    if print_args {
        for a in &parse.raw_args {
            println!("{a}");
        }
        println!();
    }
    if parse.print_help {
        let style = Style::auto();
        print!("{}", render(&style, &parse.print_help_topic, !parse.raw_args.is_empty()));
        return 0;
    }

    let mut app = App::new(settings, parse.unnamed_args);
    match app.configure() {
        Startup::Continue => {}
        Startup::Cancel { reason, exit_code } => {
            if !reason.is_empty() {
                println!("Startup Cancel: {reason}");
            }
            return exit_code;
        }
    }
    app.process()
}

impl App {
    pub fn new(settings: Settings, paths: Vec<String>) -> Self {
        Self {
            settings,
            paths,
            console: Console::new(),
            consumer_factories: default_block_consumer_factories(),
            report_factories: default_report_factories(),
            selected_consumers: Vec::new(),
            file_paths_to_skip: HashSet::new(),
            file_move: None,
            line_writer: AppendLineManager::new(),
            report_save_lock: Mutex::new(()),
        }
    }

    fn cancel(reason: &str, exit_code: i32) -> Startup {
        Startup::Cancel { reason: reason.to_string(), exit_code }
    }

    /// Validate settings and print informational listings (`InitializeSettings`).
    pub fn configure(&mut self) -> Startup {
        let s = &self.settings;

        match &s.consumers {
            None => {
                println!("Available Consumers: ");
                for f in &self.consumer_factories {
                    println!("{:<14} - {}", f.name, f.description);
                }
                return Self::cancel("", 0);
            }
            Some(list) => {
                let invalid: Vec<&str> = list.iter().filter(|c| !self.consumer_factories.iter().any(|f| f.name.eq_ignore_ascii_case(&c.name))).map(|c| c.name.as_str()).collect();
                if !invalid.is_empty() {
                    println!("Invalid BlockConsumer(s): {}", invalid.join(", "));
                    return Self::cancel("", 1);
                }
                self.selected_consumers = list.iter().map(|c| (c.name.clone(), c.arguments.clone())).collect();
            }
        }

        if s.version {
            println!("Program Version: {VERSION}");
            match crate::info::providers::mediainfo::version() {
                Some(v) => println!("{v}"),
                None => println!("MediaInfoLib: not available"),
            }
            return Self::cancel("", 0);
        }

        match &s.reports {
            None => {
                println!("Available Reports: ");
                for f in &self.report_factories {
                    println!("{:<14} - {}", f.name, f.description);
                }
                return Self::cancel("", 0);
            }
            Some(list) => {
                let invalid: Vec<&str> = list.iter().filter(|r| !self.report_factories.iter().any(|f| f.name.eq_ignore_ascii_case(r))).map(|r| r.as_str()).collect();
                if !invalid.is_empty() {
                    println!("Invalid Report: {}", invalid.join(", "));
                    return Self::cancel("", 1);
                }
            }
        }

        if s.crc32_error.as_ref().map(|(p, _)| !p.is_empty()).unwrap_or(false) && !self.selected_consumers.iter().any(|(n, _)| n.eq_ignore_ascii_case("CRC32")) {
            self.selected_consumers.push(("CRC32".to_string(), Vec::new()));
        }

        if s.print_available_simds {
            println!("Available SIMD Instructions: ");
            for name in available_simd() {
                println!("{name}");
            }
            return Self::cancel("", 0);
        }

        // Don't cancel startup when DoneLogPath doesn't exist yet.
        if !s.done_log_path.is_empty() && !Path::new(&s.done_log_path).exists() {
            if let Err(e) = create_directory_chain(&s.done_log_path, false).and_then(|_| std::fs::File::create(&s.done_log_path).map(|_| ())) {
                println!("Could not create DoneLogPath {}: {e}", s.done_log_path);
                return Self::cancel("", 1);
            }
        }

        let skip_paths = s.skip_log_paths();
        let invalid: Vec<&String> = skip_paths.iter().filter(|p| !Path::new(p).is_file()).collect();
        if invalid.is_empty() {
            for p in &skip_paths {
                if let Ok(content) = std::fs::read_to_string(p) {
                    self.file_paths_to_skip.extend(content.lines().map(|l| l.trim_end_matches('\r').to_string()));
                }
            }
        } else {
            println!("SkipLogPath contains file paths which do not exist: {}", invalid.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
            return Self::cancel("", 1);
        }

        if s.null_stream_test.stream_count > 0 && s.reports.as_ref().map(|r| !r.is_empty()).unwrap_or(false) {
            println!("NullStreamTest cannot be used with reports");
            return Self::cancel("", 1);
        }

        if s.file_move_mode != FileMoveMode::None {
            let mut script = match FileMoveScript::new(s.file_move_mode, &s.file_move_pattern) {
                Ok(sc) => sc,
                Err(e) => {
                    println!("{e}");
                    return Self::cancel("", 1);
                }
            };
            if !s.file_move_test {
                if let Err(e) = script.load() {
                    println!("{e}");
                    return Self::cancel("", 1);
                }
            } else if !script.can_reload() {
                println!("FileMove cannot enter test mode because the choosen --FileMove.Mode cannot be reloaded. It needs to be file based!");
                return Self::cancel("", 1);
            }
            self.file_move = Some(Mutex::new(script));
        }

        let s = &self.settings;
        let mut dirs: Vec<(String, bool)> = Vec::new();
        for p in s.processed_log_paths() {
            dirs.push((p, false));
        }
        for p in s.skip_log_paths() {
            dirs.push((p, false));
        }
        if let Some((p, _)) = &s.crc32_error {
            dirs.push((p.clone(), false));
        }
        dirs.push((s.extension_difference_path.clone(), false));
        if let Some(rd) = &s.report_directory {
            dirs.push((rd.clone(), true));
        }
        dirs.push((s.error_directory.clone(), true));
        for (p, is_dir) in dirs {
            if let Err(e) = create_directory_chain(&p, is_dir) {
                println!("Could not create directory for {p}: {e}");
                return Self::cancel("", 1);
            }
        }
        Startup::Continue
    }

    fn create_stream_provider(&self) -> (Box<dyn StreamProvider>, u64, usize) {
        let s = &self.settings;
        if s.null_stream_test.stream_count > 0 {
            let nsp = NullStreamProvider::new(s.null_stream_test.stream_count, s.null_stream_test.stream_length, s.null_stream_test.parallel_stream_count);
            let total = nsp.stream_count as u64 * nsp.stream_length;
            return (Box::new(nsp), total, s.null_stream_test.stream_count);
        }

        let mut accepted = 0usize;
        let mut discovery_on = Instant::now();
        let mut sp = StreamFromPathsProvider::new(&s.concurrent);
        let with_ext = &s.with_extensions;
        let skip = &self.file_paths_to_skip;
        let print_discovered = s.print_discovered_files;
        let mut accept = |path: &str| -> bool {
            let ext_ok = with_ext.items.is_empty() || with_ext.items.iter().any(|e| path.to_lowercase().ends_with(&e.to_lowercase()));
            let ok = (with_ext.allow == ext_ok) && !skip.contains(path);
            if ok {
                if print_discovered {
                    println!("Accepted file: {path}");
                } else if discovery_on.elapsed().as_secs_f64() >= 1.0 {
                    println!("Accepted files: {accepted}");
                    discovery_on = Instant::now();
                }
                accepted += 1;
            }
            ok
        };
        let mut on_error = |msg: String| println!("Filediscovery: {msg}");
        sp.add_files(&self.paths, s.recursive, &mut accept, &mut on_error);
        println!("Accepted files: {accepted}");
        println!();
        let total_bytes = sp.total_bytes;
        let total_files = sp.total_file_count;
        (Box::new(sp), total_bytes, total_files)
    }

    /// Run the whole processing pipeline. Returns the exit code.
    pub fn process(self) -> i32 {
        let app = Arc::new(self);
        let (mut provider, total_bytes, total_files) = app.create_stream_provider();

        let progress = Arc::new(BytesReadProgress::new(app.consumer_factories.iter().map(|f| f.name.clone())));
        let options = DisplayOptions { hide_buffers: app.settings.hide_buffers, hide_file_progress: app.settings.hide_file_progress, hide_total_progress: app.settings.hide_total_progress };
        let mut display = ProgressDisplay::new(Arc::clone(&progress), options, total_bytes, total_files);
        app.console.set_show_display_jitter(app.settings.show_display_jitter);
        app.console.set_renderer(Box::new(move |frame| display.write_progress(frame)));
        if !app.settings.forward_console_cursor_only {
            app.console.start_progress_display();
        }

        let ct = CancelToken::new();
        {
            let ct = ct.clone();
            let _ = ctrlc::set_handler(move || ct.cancel());
        }

        let pool = Arc::new(MirroredBufferPool::new(app.settings.buffer_length));
        let factory = Arc::new(StreamConsumerFactory::new(
            app.consumer_factories.clone(),
            app.selected_consumers.clone(),
            pool,
            app.settings.producer_min_read_length,
            app.settings.producer_max_read_length,
        ));
        let collection = StreamConsumerCollection::new(factory);

        let on_error: Arc<dyn Fn(&ProcessingError, usize) -> bool + Send + Sync> = {
            let app = Arc::clone(&app);
            Arc::new(move |e, retry_count| {
                let file = e.data.iter().find(|(k, _)| k == "StreamTag").map(|(_, v)| crate::misc::file_name(Path::new(v))).unwrap_or_default();
                app.on_exception(AvdError::ui("ConsumingStream").with_cause(AvdError::from_processing(e)).with_sensitive("FileName", file));
                retry_count < 2
            })
        };
        let on_finished: Arc<dyn Fn(StreamResult) + Send + Sync> = {
            let app = Arc::clone(&app);
            Arc::new(move |result| app.handle_stream_result(result))
        };

        let result = collection.consume_streams(provider.as_mut(), progress, &ct, on_error, on_finished);
        drop(provider);

        let mut exit_code = 0;
        match result {
            Ok(()) => {
                if app.console.showing_progress() {
                    app.console.stop_progress_display();
                }
                app.console.write_display_progress();
            }
            Err(e) if e.is_cancelled() => {
                if app.console.showing_progress() {
                    app.console.stop_progress_display();
                }
                println!("Processing was cancelled.");
                exit_code = 130;
            }
            Err(e) => {
                if app.console.showing_progress() {
                    app.console.stop_progress_display();
                }
                app.on_exception(AvdError::from_processing(&e));
                exit_code = 1;
            }
        }
        app.line_writer.clear();

        if app.settings.pause_before_exit {
            println!("Program execution has finished. Press any key to exit.");
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
        }
        exit_code
    }

    /// Print (and optionally save) an error (`OnException`).
    pub fn on_exception(&self, err: AvdError) {
        let s = &self.settings;
        if s.save_errors {
            let xml = err.to_xml(s.skip_environment_element, s.include_personal_data, if s.include_personal_data { &s.effective_args } else { &[] });
            let file_name = format!("AVD3Error{}.xml", error_file_stamp(err.thrown_on));
            let path = Path::new(&s.error_directory).join(file_name);
            let _ = std::fs::create_dir_all(&s.error_directory);
            let _ = std::fs::write(&path, xml.to_string_indented());
        }
        let base = err.base();
        self.console.write_line(&format!("Error {}: {}", base.type_name, base.message));
        if let Some(r) = &err.remedy {
            self.console.write_line(r);
        }
    }

    fn handle_stream_result(&self, result: StreamResult) {
        let file_path = result.tag.clone();
        let file_name = crate::misc::file_name(Path::new(&file_path));

        // Consumers that failed are left out of the metadata (their results are undefined).
        let consumers: Vec<Box<dyn BlockConsumer>> = result.outcomes.into_iter().filter(|o| o.error.is_none()).map(|o| o.consumer).collect();

        let fmi = match self.create_file_meta_info(&file_path, &consumers) {
            Some(f) => f,
            None => return,
        };
        let mut success = self.handle_reporting(&fmi, &consumers);
        success = success && self.handle_file_move(&fmi);

        if success {
            for p in self.settings.processed_log_paths() {
                if let Err(e) = self.line_writer.append_line(&p, &fmi.full_name()) {
                    self.on_exception(AvdError::ui(format!("Couldn't write to processed log: {e}")).with_sensitive("FileName", file_name.clone()));
                }
            }
        }
    }

    fn create_file_meta_info(&self, file_path: &str, consumers: &[Box<dyn BlockConsumer>]) -> Option<FileMetaInfo> {
        let path = Path::new(file_path);
        let setup = InfoProviderSetup { file_path: path, block_consumers: consumers };
        let providers: Vec<_> = default_info_provider_factories().iter().filter_map(|f| f.create(&setup)).collect();
        Some(FileMetaInfo::new(path, providers))
    }

    fn handle_reporting(&self, fmi: &FileMetaInfo, consumers: &[Box<dyn BlockConsumer>]) -> bool {
        let s = &self.settings;
        let file_name = fmi.file_name();
        let mut lines: Vec<String> = Vec::new();
        if s.print_hashes || s.print_reports {
            lines.push(file_name.clone());
        }
        if s.print_hashes {
            if let Some(hp) = fmi.provider("HashProvider") {
                for item in &hp.root.items {
                    if let Some(b) = item.value.as_bytes() {
                        lines.push(format!("{} => {}", item.key, crate::hashes::to_hex_upper(b)));
                    }
                }
            }
            lines.push(String::new());
        }

        if let Some((path, pattern)) = &s.crc32_error {
            if !path.is_empty() {
                if let Some(crc) = fmi.hash("CRC32") {
                    let crc_str = crate::hashes::to_hex_upper(crc);
                    let matched = Regex::new(&pattern.replace("${CRC32}", &crc_str)).map(|re| re.is_match(&fmi.full_name())).unwrap_or(true);
                    if !matched {
                        let _ = self.line_writer.append_line(path, &format!("{} {}", crc_str, fmi.full_name()));
                    }
                }
            }
        }

        if !s.extension_difference_path.is_empty() {
            let mut det_exts: Vec<String> = fmi.suggested_extensions().iter().flat_map(|e| e.split(' ').map(|x| x.to_string())).filter(|e| !e.is_empty()).collect();
            let ext = fmi.extension();
            let ext = ext.strip_prefix('.').unwrap_or(&ext).to_string();
            if !det_exts.iter().any(|e| e.eq_ignore_ascii_case(&ext)) {
                if det_exts.is_empty() {
                    det_exts.push("unknown".to_string());
                }
                let _ = self.line_writer.append_line(&s.extension_difference_path, &format!("{} => {}\t{}", ext, det_exts.join(" "), fmi.full_name()));
            }
        }

        let mut success = true;
        let selected: Vec<&ReportFactory> = self.report_factories.iter().filter(|f| s.reports.as_ref().map(|r| r.iter().any(|x| x.eq_ignore_ascii_case(&f.name))).unwrap_or(false)).collect();
        if !selected.is_empty() {
            let mut tokens: HashMap<String, String> = HashMap::new();
            for factory in selected {
                let report = factory.create(fmi, consumers);
                if s.print_reports {
                    lines.push(format!("{}\n", report.to_report_string()));
                }
                tokens.insert("ReportName".to_string(), factory.name.clone());
                tokens.insert("ReportFileExtension".to_string(), report.file_extension.to_string());
                let report_file_name = expand_placeholders(&s.report_file_name, fmi, Some(&tokens));
                let prefix = expand_placeholders(&s.report_content_prefix, fmi, Some(&tokens));
                if let Some(dir) = &s.report_directory {
                    let _guard = self.report_save_lock.lock().unwrap_or_else(|e| e.into_inner());
                    if let Err(e) = report.save_to_file(&Path::new(dir).join(&report_file_name), &prefix) {
                        self.on_exception(AvdError::ui("GeneratingReports").with_cause(AvdError::new("IOException", e.to_string())).with_sensitive("FileName", file_name.clone()));
                        success = false;
                    }
                }
            }
        }
        self.console.write_lines(&lines);
        success
    }

    fn handle_file_move(&self, fmi: &FileMetaInfo) -> bool {
        let script = match &self.file_move {
            Some(sc) => sc,
            None => return true,
        };
        let s = &self.settings;
        let mut script = script.lock().unwrap_or_else(|e| e.into_inner());
        let _console_lock = if s.file_move_test { Some(self.console.lock_console()) } else { None };

        let mut move_file = true;
        let mut action_key = ' ';
        let mut repeat = s.file_move_test;
        loop {
            let mut dest: Option<String> = None;
            let load_ok = if s.file_move_test { script.load().is_ok() } else { true };
            if load_ok {
                if let Some(d) = script.get_file_path(fmi) {
                    dest = Some(finalize_destination(d, fmi, &s.file_move_replacements, s.disable_file_move, s.disable_file_rename));
                }
            }

            if s.file_move_test {
                println!();
                println!();
                println!("FileMove.Test Enabled{}{}", if s.disable_file_move { " (DisableFileMove Enabled!)" } else { "" }, if s.disable_file_rename { " (DisableFileRename Enabled!)" } else { "" });
                println!("Directoryname: ");
                println!("Old: {}", fmi.directory_name());
                println!("New: {}", dest.as_ref().map(|d| crate::misc::directory_name(Path::new(d))).unwrap_or_default());
                println!("Filename: ");
                println!("Old: {}", fmi.file_name());
                println!("New: {}", dest.as_ref().map(|d| crate::misc::file_name(Path::new(d))).unwrap_or_default());

                if action_key == 'A' {
                    println!("Press any key to cancel automatic mode");
                    let mut key_pressed = false;
                    loop {
                        if key_available(500) {
                            key_pressed = true;
                            break;
                        }
                        if script.source_changed() {
                            break;
                        }
                    }
                    if !key_pressed {
                        continue;
                    }
                    let _ = read_key();
                }

                loop {
                    println!();
                    println!("How do you wish to continue?");
                    println!("(C) Continue without moving the file");
                    println!("(R) Repeat script execution");
                    println!("(A) Repeat script execution automatically on sourcefile change");
                    println!("(M) Moving the file and continue");
                    print!("User Input: ");
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                    action_key = read_key().map(|c| c.to_ascii_uppercase()).unwrap_or('C');
                    println!();
                    println!();
                    if matches!(action_key, 'C' | 'R' | 'A' | 'M') {
                        break;
                    }
                }
                move_file = action_key == 'M';
                repeat = action_key == 'R' || action_key == 'A';
            }

            if move_file {
                if let Some(dest) = &dest {
                    if !dest.is_empty() && dest != &fmi.full_name() {
                        let original = fmi.full_name();
                        if let Err(e) = move_path(&original, dest) {
                            self.on_exception(AvdError::ui("FileMove").with_cause(AvdError::new("IOException", e.to_string())).with_sensitive("FileName", fmi.file_name()));
                            return false;
                        }
                        if !s.file_move_log_path.is_empty() {
                            let _ = self.line_writer.append_line(&s.file_move_log_path, &format!("{original} => {dest}"));
                        }
                    }
                }
            }
            if !repeat {
                break;
            }
        }
        true
    }
}

fn move_path(from: &str, to: &str) -> std::io::Result<()> {
    if let Some(parent) = Path::new(to).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    if Path::new(to).exists() {
        return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, format!("Cannot move to {to}: file exists")));
    }
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Cross-device: copy then delete.
            std::fs::copy(from, to)?;
            std::fs::remove_file(from)
        }
    }
}

fn key_available(timeout_ms: u64) -> bool {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        std::thread::sleep(std::time::Duration::from_millis(timeout_ms));
        return false;
    }
    let _ = crossterm::terminal::enable_raw_mode();
    let available = crossterm::event::poll(std::time::Duration::from_millis(timeout_ms)).unwrap_or(false);
    let _ = crossterm::terminal::disable_raw_mode();
    available
}

fn read_key() -> Option<char> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).ok()?;
        return line.trim().chars().next();
    }
    let _ = crossterm::terminal::enable_raw_mode();
    let key = loop {
        match crossterm::event::read() {
            Ok(crossterm::event::Event::Key(k)) if k.kind == crossterm::event::KeyEventKind::Press => {
                break match k.code {
                    crossterm::event::KeyCode::Char(c) => Some(c),
                    crossterm::event::KeyCode::Enter => Some('\n'),
                    _ => Some(' '),
                };
            }
            Ok(_) => continue,
            Err(_) => break None,
        }
    };
    let _ = crossterm::terminal::disable_raw_mode();
    if let Some(c) = key {
        print!("{c}");
    }
    key
}

/// CPU SIMD features that are available (names as in the original `CPUInstructions` enum).
pub fn available_simd() -> Vec<&'static str> {
    let mut v = Vec::new();
    #[cfg(target_arch = "x86_64")]
    {
        v.push("x64");
        macro_rules! feat {
            ($name:tt, $display:literal) => {
                if std::arch::is_x86_feature_detected!($name) {
                    v.push($display);
                }
            };
        }
        feat!("mmx", "MMX");
        feat!("lzcnt", "ABM");
        feat!("rdrand", "RDRAND");
        feat!("bmi1", "BMI1");
        feat!("bmi2", "BMI2");
        feat!("adx", "ADX");
        feat!("sse", "SSE");
        feat!("sse2", "SSE2");
        feat!("sse3", "SSE3");
        feat!("ssse3", "SSSE3");
        feat!("sse4.1", "SSE41");
        feat!("sse4.2", "SSE42");
        feat!("sse4a", "SSE4a");
        feat!("aes", "AES");
        feat!("sha", "SHA");
        feat!("avx", "AVX");
        feat!("fma", "FMA3");
        feat!("avx2", "AVX2");
        feat!("avx512f", "AVX512F");
        feat!("avx512cd", "AVX512CD");
        feat!("avx512vl", "AVX512VL");
        feat!("avx512bw", "AVX512BW");
        feat!("avx512dq", "AVX512DQ");
        feat!("avx512ifma", "AVX512IFMA");
        feat!("avx512vbmi", "AVX512VBMI");
    }
    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("neon") {
            v.push("NEON");
        }
        if std::arch::is_aarch64_feature_detected!("sha2") {
            v.push("SHA");
        }
        if std::arch::is_aarch64_feature_detected!("aes") {
            v.push("AES");
        }
        if std::arch::is_aarch64_feature_detected!("crc") {
            v.push("CRC");
        }
    }
    if v.is_empty() {
        v.push("Couldn't fetch cpu instructions!");
    }
    v
}

/// `MediaProvider` container type name, re-exported for tests.
pub const MEDIA_PROVIDER: &str = container_types::MEDIA_PROVIDER;
