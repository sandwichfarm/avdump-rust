//! Command line settings: the property registry (names, aliases, descriptions, defaults) and
//! the typed [`Settings`] struct they are parsed into.

pub mod cli;
pub mod help;

use crate::processing::stream_provider::{PathPartition, PathPartitions};
use regex::Regex;

/// Setting namespaces, in help order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    FileDiscovery,
    Processing,
    FileMove,
    Reporting,
    Diagnostics,
    Display,
}

impl Group {
    pub const ALL: [Group; 6] = [Group::FileDiscovery, Group::Processing, Group::FileMove, Group::Reporting, Group::Diagnostics, Group::Display];

    pub fn name(&self) -> &'static str {
        match self {
            Group::FileDiscovery => "FileDiscovery",
            Group::Processing => "Processing",
            Group::FileMove => "FileMove",
            Group::Reporting => "Reporting",
            Group::Diagnostics => "Diagnostics",
            Group::Display => "Display",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Group::FileDiscovery => "Which files are processed and how many at once",
            Group::Processing => "Buffering and the consumers (hashes/parsers) that run over each file",
            Group::FileMove => "Move or rename files after processing using placeholders",
            Group::Reporting => "Reports and side logs generated per file",
            Group::Diagnostics => "Version info, error files and speed tests",
            Group::Display => "Console progress display",
        }
    }
}

/// Value shape of a property (drives default rendering and boolean switch handling).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Bool,
    Text,
}

#[derive(Debug, Clone)]
pub struct SettingProperty {
    pub group: Group,
    pub name: &'static str,
    pub alternative_names: &'static [&'static str],
    pub kind: ValueKind,
    pub description: &'static str,
    pub example: &'static str,
    /// Default value as shown in the help (`None` renders as `<null>`, empty as `<Empty>`).
    pub default_display: Option<String>,
}

impl SettingProperty {
    pub fn full_name(&self) -> String {
        format!("{}.{}", self.group.name(), self.name)
    }

    /// `--Name, --Alt, -A` rendering.
    pub fn names_display(&self) -> String {
        std::iter::once(self.name)
            .chain(self.alternative_names.iter().copied())
            .map(|n| if n.chars().count() == 1 { format!("-{n}") } else { format!("--{n}") })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

const fn prop(group: Group, name: &'static str, alternative_names: &'static [&'static str], kind: ValueKind, description: &'static str, example: &'static str) -> (Group, &'static str, &'static [&'static str], ValueKind, &'static str, &'static str) {
    (group, name, alternative_names, kind, description, example)
}

/// All registered properties in help order.
pub fn all_properties() -> Vec<SettingProperty> {
    use Group::*;
    use ValueKind::*;
    let cwd = std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let defs = [
        prop(FileDiscovery, "Recursive", &["R"], Bool, "Recursively descent into Subdirectories", "--Recursive"),
        prop(FileDiscovery, "ProcessedLogPath", &["PLPath"], Text, "Appends the full filepath to the specified path", "--ProcessedLogPath=<FilePath1>[,<FilePath2>...]"),
        prop(FileDiscovery, "SkipLogPath", &["SLPath"], Text, "Filepaths contained in the specified file will not be processed", "--SkipLogPath=<FilePath1>[,<FilePath2>...]"),
        prop(FileDiscovery, "DoneLogPath", &["DLPath"], Text, "Will set --SkipLogPath and --ProcessedLogPath to the specified filepath", "--DoneLogPath=<Filepath>"),
        prop(FileDiscovery, "WithExtensions", &["WExts"], Text, "Only/Don't Process files with selected Extensions", "--WithExtensions=[-]<Extension1>[,<Extension2>...]"),
        prop(FileDiscovery, "Concurrent", &["Conc"], Text, "Sets the maximal number of files which will be processed concurrently.\nFirst param (max) sets a global limit. (path,max) pairs sets limits per path.", "--Concurrent=<max>[:<path1>,<max1>;<path2>,<max2>...]"),
        prop(Processing, "ProducerMinReadLength", &[], Text, "How much data in MiB the reader has to read each time at minimum", "--ProducerMinReadLength=<Size in MiB>"),
        prop(Processing, "ProducerMaxReadLength", &[], Text, "How much data in MiB the reader is allowed to read each time at most", "--ProducerMaxReadLength=<Size in MiB>"),
        prop(Processing, "PrintAvailableSIMDs", &[], Bool, "Print available CPU SIMDs", "--PrintAvailableSIMDs"),
        prop(Processing, "PauseBeforeExit", &["PBExit"], Bool, "Pause console before exiting", "--PauseBeforeExit"),
        prop(Processing, "BufferLength", &["BLength"], Text, "Circular buffer size for hashing", "--BufferLength=<Size in MiB>"),
        prop(Processing, "Consumers", &["Cons"], Text, "Select consumers to use. Use without arguments to list available consumers.\nArguments can be passed per consumer: NAME:arg1|arg2 (e.g. TTH:4, NULL:8, CPY:<Directory>)", "--Consumers=<ConsumerName1>[,<ConsumerName2>...]"),
        prop(FileMove, "Test", &[], Bool, "Test FileMove Settings", "--FileMove.Test"),
        prop(FileMove, "LogPath", &[], Text, "A line is written for each file that has been moved/renamed. (OldPath => NewPath)", "--FileMove.LogPath=<FilePath>"),
        prop(FileMove, "Mode", &[], Text, "Determines how the Pattern Argument is going to be interpreted:\nInline: Script is directly entered as the argument\nFile: A path pointing to the script file\nPlaceholder: See example for --Pattern\nCSharpScript/DotNetAssembly: Not supported by this port (kept for command compatibility)", "--FileMove.Mode=<None|PlaceholderInline|PlaceholderFile|CSharpScriptInline|CSharpScriptFile|DotNetAssembly>"),
        prop(FileMove, "Pattern", &[], Text, "Available Placeholders in the form of ${Name}:\nFileSize, FullName, FileName, FileExtension, FileNameWithoutExtension, DirectoryName, SuggestedExtension,\nHash-<Name>-<2|4|8|10|16|32|32Hex|32Z|36|62|64>-<OC|UC|LC>", "--FileMove.Pattern=${DirectoryName}/${FileNameWithoutExtension}${SuggestedExtension}"),
        prop(FileMove, "DisableFileMove", &[], Bool, "Don't move the file even if the Pattern says so", "--FileMove.DisableFileMove"),
        prop(FileMove, "DisableFileRename", &[], Bool, "Don't rename the file even if the Pattern says so", "--FileMove.DisableFileRename"),
        prop(FileMove, "Replacements", &[], Text, "Replace substrings in the returned filepath", "--FileMove.Replacements=<Match1>=<Replacement1>[;<Match2>=<Replacement2>...]"),
        prop(Reporting, "PrintHashes", &[], Bool, "Print calculated hashes in hexadecimal format to console", "--PrintHashes"),
        prop(Reporting, "PrintReports", &[], Bool, "Print generated reports to console", "--PrintReports"),
        prop(Reporting, "Reports", &[], Text, "Select reports to use. Use without arguments to list available reports", "--Reports=<ReportName1>[,<ReportName2>...]"),
        prop(Reporting, "ReportDirectory", &["RDir"], Text, "Reports will be saved to the specified directory", "--ReportDirectory=<Directory>"),
        prop(Reporting, "ReportFileName", &[], Text, "Reports will be saved/appended to the specified filename\nPlaceholders mentioned in --FileMove.Pattern can be used as well\nAdditional placeholders: ReportName, ReportFileExtension", "--ReportFileName=<FileName>"),
        prop(Reporting, "ReportContentPrefix", &[], Text, "Each report will be prefixed with the arguments content\nSee ReportFileName for Placeholders", "--ReportContentPrefix=${FullName}"),
        prop(Reporting, "ExtensionDifferencePath", &["EDPath"], Text, "Logs the filepath if the detected extension does not match the actual extension", "--EDPath=extdiff.txt"),
        prop(Reporting, "CRC32Error", &[], Text, "Searches the filename for the calculated CRC32 hash. If not present or different a line with the calculated hash and the full path of the file is appended to the specified path\nThe regex pattern should contain the placeholder ${CRC32} which is replaced by the calculated hash prior matching.\nConsumer CRC32 will be force enabled!", "--CRC32Error=<Filepath>,<RegexPattern>"),
        prop(Diagnostics, "Version", &[], Bool, "Print the program version to console", "--Version"),
        prop(Diagnostics, "SaveErrors", &[], Bool, "Errors occuring during program execution will be saved to disk", "--SaveErrors"),
        prop(Diagnostics, "SkipEnvironmentElement", &[], Bool, "Skip the environment element in error files", "--SkipEnvironmentElement"),
        prop(Diagnostics, "IncludePersonalData", &[], Bool, "Various places may include personal data. Currently this only affects error files, which will then include the full filepath", "--IncludePersonalData"),
        prop(Diagnostics, "PrintDiscoveredFiles", &[], Bool, "Print each discovered file path instead of printing the count", "--PrintDiscoveredFiles"),
        prop(Diagnostics, "ErrorDirectory", &[], Text, "If --SaveErrors is specified the error files will be placed in the specified path", "--ErrorDirectory=<DirectoryPath>"),
        prop(Diagnostics, "NullStreamTest", &[], Text, "Use Memory as the DataSource for HashSpeed testing. Overrides any FileDiscovery Settings!", "--NullStreamTest=<StreamCount>:<StreamLength in MiB>:<ParallelStreamCount>"),
        prop(Display, "HideBuffers", &[], Bool, "Hides buffer bars", "--HideBuffers"),
        prop(Display, "HideFileProgress", &[], Bool, "Hides file progress", "--HideFileProgress"),
        prop(Display, "HideTotalProgress", &[], Bool, "Hides total progress", "--HideTotalProgress"),
        prop(Display, "ShowDisplayJitter", &[], Bool, "Displays the time taken to calculate progression stats and drawing to console", "--ShowDisplayJitter"),
        prop(Display, "ForwardConsoleCursorOnly", &[], Bool, "The cursor position of the console will not be explicitly set. This option will disable most progress output", "--ForwardConsoleCursorOnly"),
    ];
    defs.iter()
        .map(|(group, name, alt, kind, description, example)| {
            let default_display = match (group, *name) {
                (_, _) if *kind == ValueKind::Bool => Some("False".to_string()),
                (Group::FileDiscovery, "ProcessedLogPath") | (Group::FileDiscovery, "SkipLogPath") => Some("{}".to_string()),
                (Group::FileDiscovery, "Concurrent") => Some("1".to_string()),
                (Group::Processing, "ProducerMinReadLength") => Some("1".to_string()),
                (Group::Processing, "ProducerMaxReadLength") => Some("8".to_string()),
                (Group::Processing, "BufferLength") => Some("64".to_string()),
                (Group::Processing, "Consumers") | (Group::Reporting, "Reports") | (Group::Reporting, "ReportDirectory") => None,
                (Group::FileMove, "Mode") => Some("None".to_string()),
                (Group::FileMove, "Pattern") => Some(format!("${{DirectoryName}}{}${{FileNameWithoutExtension}}${{FileExtension}}", std::path::MAIN_SEPARATOR)),
                (Group::Reporting, "ReportFileName") => Some("${FileName}.${ReportName}.${ReportFileExtension}".to_string()),
                (Group::Reporting, "CRC32Error") => Some(",(?i)${CRC32}".to_string()),
                (Group::Diagnostics, "ErrorDirectory") => Some(cwd.clone()),
                (Group::Diagnostics, "NullStreamTest") => Some("0:0:0".to_string()),
                _ => Some(String::new()),
            };
            SettingProperty { group: *group, name, alternative_names: alt, kind: *kind, description, example, default_display }
        })
        .collect()
}

// ------------------------------------------------------------------------------ typed settings

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileMoveMode {
    None,
    PlaceholderInline,
    PlaceholderFile,
    CSharpScriptInline,
    CSharpScriptFile,
    DotNetAssembly,
}

impl FileMoveMode {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "none" => Self::None,
            "placeholderinline" => Self::PlaceholderInline,
            "placeholderfile" => Self::PlaceholderFile,
            "csharpscriptinline" => Self::CSharpScriptInline,
            "csharpscriptfile" => Self::CSharpScriptFile,
            "dotnetassembly" => Self::DotNetAssembly,
            _ => return None,
        })
    }
    pub fn name(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::PlaceholderInline => "PlaceholderInline",
            Self::PlaceholderFile => "PlaceholderFile",
            Self::CSharpScriptInline => "CSharpScriptInline",
            Self::CSharpScriptFile => "CSharpScriptFile",
            Self::DotNetAssembly => "DotNetAssembly",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConsumerSetting {
    pub name: String,
    pub arguments: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileExtensionsSetting {
    pub allow: bool,
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NullStreamTestSettings {
    pub stream_count: usize,
    pub stream_length: u64,
    pub parallel_stream_count: usize,
}

#[derive(Debug, Clone)]
pub struct Settings {
    // FileDiscovery
    pub recursive: bool,
    processed_log_path: Vec<String>,
    skip_log_path: Vec<String>,
    pub done_log_path: String,
    pub with_extensions: FileExtensionsSetting,
    pub concurrent: PathPartitions,
    // Processing
    pub producer_min_read_length: usize,
    pub producer_max_read_length: usize,
    pub print_available_simds: bool,
    pub pause_before_exit: bool,
    pub buffer_length: usize,
    /// `None` means "list consumers" (`--Consumers` without a value).
    pub consumers: Option<Vec<ConsumerSetting>>,
    // FileMove
    pub file_move_test: bool,
    pub file_move_log_path: String,
    pub file_move_mode: FileMoveMode,
    pub file_move_pattern: String,
    pub disable_file_move: bool,
    pub disable_file_rename: bool,
    pub file_move_replacements: Vec<(String, String)>,
    // Reporting
    pub print_hashes: bool,
    pub print_reports: bool,
    /// `None` means "list reports".
    pub reports: Option<Vec<String>>,
    pub report_directory: Option<String>,
    pub report_file_name: String,
    pub report_content_prefix: String,
    pub extension_difference_path: String,
    pub crc32_error: Option<(String, String)>,
    // Diagnostics
    pub version: bool,
    pub save_errors: bool,
    pub skip_environment_element: bool,
    pub include_personal_data: bool,
    pub print_discovered_files: bool,
    pub error_directory: String,
    pub null_stream_test: NullStreamTestSettings,
    // Display
    pub hide_buffers: bool,
    pub hide_file_progress: bool,
    pub hide_total_progress: bool,
    pub show_display_jitter: bool,
    pub forward_console_cursor_only: bool,
    /// `Group.Name=value` of every explicitly set property (for error files).
    pub effective_args: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            recursive: false,
            processed_log_path: Vec::new(),
            skip_log_path: Vec::new(),
            done_log_path: String::new(),
            with_extensions: FileExtensionsSetting { allow: true, items: Vec::new() },
            concurrent: PathPartitions::new(1, Vec::new()),
            producer_min_read_length: 1 << 20,
            producer_max_read_length: 8 << 20,
            print_available_simds: false,
            pause_before_exit: false,
            buffer_length: 64 << 20,
            consumers: Some(Vec::new()),
            file_move_test: false,
            file_move_log_path: String::new(),
            file_move_mode: FileMoveMode::None,
            file_move_pattern: format!("${{DirectoryName}}{}${{FileNameWithoutExtension}}${{FileExtension}}", std::path::MAIN_SEPARATOR),
            disable_file_move: false,
            disable_file_rename: false,
            file_move_replacements: Vec::new(),
            print_hashes: false,
            print_reports: false,
            reports: Some(Vec::new()),
            report_directory: None,
            report_file_name: "${FileName}.${ReportName}.${ReportFileExtension}".to_string(),
            report_content_prefix: String::new(),
            extension_difference_path: String::new(),
            crc32_error: None,
            version: false,
            save_errors: false,
            skip_environment_element: false,
            include_personal_data: false,
            print_discovered_files: false,
            error_directory: std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
            null_stream_test: NullStreamTestSettings { stream_count: 0, stream_length: 0, parallel_stream_count: 0 },
            hide_buffers: false,
            hide_file_progress: false,
            hide_total_progress: false,
            show_display_jitter: false,
            forward_console_cursor_only: false,
            effective_args: Vec::new(),
        }
    }
}

fn dedupe_ci(mut v: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    v.retain(|s| !s.is_empty());
    for s in v {
        if !out.iter().any(|x| x.eq_ignore_ascii_case(&s)) {
            out.push(s);
        }
    }
    out
}

impl Settings {
    /// Paths to append processed files to (includes `DoneLogPath`).
    pub fn processed_log_paths(&self) -> Vec<String> {
        let mut v = self.processed_log_path.clone();
        v.push(self.done_log_path.clone());
        dedupe_ci(v)
    }

    /// Paths of files listing paths to skip (includes `DoneLogPath`).
    pub fn skip_log_paths(&self) -> Vec<String> {
        let mut v = self.skip_log_path.clone();
        v.push(self.done_log_path.clone());
        dedupe_ci(v)
    }

    fn parse_int(value: &str, what: &str) -> Result<i64, String> {
        value.trim().parse::<i64>().map_err(|_| format!("{what}: '{value}' is not a valid integer"))
    }

    /// Apply one parsed argument. `value` is `None` when the argument was given without `=`.
    pub fn apply(&mut self, property: &SettingProperty, value: Option<&str>) -> Result<(), String> {
        let raw = value.map(|s| s.to_string());
        let bool_value = || -> Result<bool, String> {
            match value {
                None => Ok(true),
                Some(v) => match v.trim().to_ascii_lowercase().as_str() {
                    "true" | "1" | "yes" | "on" => Ok(true),
                    "false" | "0" | "no" | "off" => Ok(false),
                    _ => Err(format!("'{v}' is not a valid boolean")),
                },
            }
        };
        let text = || value.unwrap_or("");
        let list = |sep: &[char]| -> Vec<String> { text().split(sep).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect() };
        let mib = |what: &str| -> Result<usize, String> {
            let v = Self::parse_int(text(), what)?;
            if v <= 0 {
                return Err(format!("{what}: must be a positive number of MiB"));
            }
            Ok((v as usize) << 20)
        };

        match (property.group, property.name) {
            (Group::FileDiscovery, "Recursive") => self.recursive = bool_value()?,
            (Group::FileDiscovery, "ProcessedLogPath") => self.processed_log_path = text().split(',').map(|s| s.trim().to_string()).collect(),
            (Group::FileDiscovery, "SkipLogPath") => self.skip_log_path = text().split(',').map(|s| s.trim().to_string()).collect(),
            (Group::FileDiscovery, "DoneLogPath") => self.done_log_path = text().to_string(),
            (Group::FileDiscovery, "WithExtensions") => {
                let s = value;
                let allow = match s {
                    None => true,
                    Some(s) => !s.starts_with('-'),
                };
                let body = match s {
                    Some(s) if !allow => &s[1..],
                    Some(s) => s,
                    None => "",
                };
                self.with_extensions = FileExtensionsSetting { allow, items: body.split([',', ' ']).filter(|x| !x.is_empty()).map(|x| x.to_string()).collect() };
            }
            (Group::FileDiscovery, "Concurrent") => {
                let mut parts = text().splitn(2, ':');
                let max = Self::parse_int(parts.next().unwrap_or(""), "Concurrent")?;
                if max <= 0 {
                    return Err("Concurrent: must be at least 1".into());
                }
                let mut partitions = Vec::new();
                if let Some(rest) = parts.next() {
                    for item in rest.split(';').filter(|s| !s.is_empty()) {
                        let mut kv = item.splitn(2, ',');
                        let path = kv.next().unwrap_or("").to_string();
                        let count = Self::parse_int(kv.next().ok_or_else(|| format!("Concurrent: missing count for path '{path}'"))?, "Concurrent")?;
                        partitions.push(PathPartition { path, concurrent_count: count.max(1) as usize });
                    }
                }
                self.concurrent = PathPartitions::new(max as usize, partitions);
            }
            (Group::Processing, "ProducerMinReadLength") => self.producer_min_read_length = mib("ProducerMinReadLength")?,
            (Group::Processing, "ProducerMaxReadLength") => self.producer_max_read_length = mib("ProducerMaxReadLength")?,
            (Group::Processing, "PrintAvailableSIMDs") => self.print_available_simds = bool_value()?,
            (Group::Processing, "PauseBeforeExit") => self.pause_before_exit = bool_value()?,
            (Group::Processing, "BufferLength") => self.buffer_length = mib("BufferLength")?,
            (Group::Processing, "Consumers") => {
                self.consumers = match value {
                    None | Some("") => None,
                    Some(s) => Some(
                        s.split(',')
                            .filter(|x| !x.trim().is_empty())
                            .map(|x| {
                                let mut parts = x.splitn(2, ':');
                                let name = parts.next().unwrap_or("").trim().to_string();
                                let arguments = parts.next().map(|a| a.split('|').map(|y| y.to_string()).collect()).unwrap_or_default();
                                ConsumerSetting { name, arguments }
                            })
                            .collect(),
                    ),
                };
            }
            (Group::FileMove, "Test") => self.file_move_test = bool_value()?,
            (Group::FileMove, "LogPath") => self.file_move_log_path = text().to_string(),
            (Group::FileMove, "Mode") => self.file_move_mode = FileMoveMode::parse(text()).ok_or_else(|| format!("'{}' is not a valid FileMove.Mode", text()))?,
            (Group::FileMove, "Pattern") => self.file_move_pattern = text().to_string(),
            (Group::FileMove, "DisableFileMove") => self.disable_file_move = bool_value()?,
            (Group::FileMove, "DisableFileRename") => self.disable_file_rename = bool_value()?,
            (Group::FileMove, "Replacements") => {
                let mut out = Vec::new();
                for item in text().split(';').filter(|s| !s.is_empty()) {
                    let mut kv = item.splitn(2, '=');
                    let k = kv.next().unwrap_or("").to_string();
                    let v = kv.next().ok_or_else(|| format!("Replacements: '{item}' is missing '='"))?.to_string();
                    out.push((k, v));
                }
                self.file_move_replacements = out;
            }
            (Group::Reporting, "PrintHashes") => self.print_hashes = bool_value()?,
            (Group::Reporting, "PrintReports") => self.print_reports = bool_value()?,
            (Group::Reporting, "Reports") => {
                self.reports = match value {
                    None | Some("") => None,
                    Some(_) => Some(list(&[','])),
                };
            }
            (Group::Reporting, "ReportDirectory") => self.report_directory = Some(text().to_string()),
            (Group::Reporting, "ReportFileName") => self.report_file_name = text().to_string(),
            (Group::Reporting, "ReportContentPrefix") => self.report_content_prefix = text().to_string(),
            (Group::Reporting, "ExtensionDifferencePath") => self.extension_difference_path = text().to_string(),
            (Group::Reporting, "CRC32Error") => {
                let mut parts = text().splitn(2, ',');
                let path = parts.next().unwrap_or("").to_string();
                let pattern = parts.next().map(|s| s.to_string()).unwrap_or_else(|| "(?i)${CRC32}".to_string());
                // Throw early on invalid regex.
                Regex::new(&pattern.replace("${CRC32}", "12345678")).map_err(|e| format!("CRC32Error: invalid regex pattern: {e}"))?;
                self.crc32_error = Some((path, pattern));
            }
            (Group::Diagnostics, "Version") => self.version = bool_value()?,
            (Group::Diagnostics, "SaveErrors") => self.save_errors = bool_value()?,
            (Group::Diagnostics, "SkipEnvironmentElement") => self.skip_environment_element = bool_value()?,
            (Group::Diagnostics, "IncludePersonalData") => self.include_personal_data = bool_value()?,
            (Group::Diagnostics, "PrintDiscoveredFiles") => self.print_discovered_files = bool_value()?,
            (Group::Diagnostics, "ErrorDirectory") => self.error_directory = text().to_string(),
            (Group::Diagnostics, "NullStreamTest") => {
                let parts: Vec<&str> = text().split(':').collect();
                if parts.len() != 3 {
                    return Err("NullStreamTest: expected <StreamCount>:<StreamLength in MiB>:<ParallelStreamCount>".into());
                }
                self.null_stream_test = NullStreamTestSettings {
                    stream_count: Self::parse_int(parts[0], "NullStreamTest")?.max(0) as usize,
                    stream_length: (Self::parse_int(parts[1], "NullStreamTest")?.max(0) as u64) << 20,
                    parallel_stream_count: Self::parse_int(parts[2], "NullStreamTest")?.max(0) as usize,
                };
            }
            (Group::Display, "HideBuffers") => self.hide_buffers = bool_value()?,
            (Group::Display, "HideFileProgress") => self.hide_file_progress = bool_value()?,
            (Group::Display, "HideTotalProgress") => self.hide_total_progress = bool_value()?,
            (Group::Display, "ShowDisplayJitter") => self.show_display_jitter = bool_value()?,
            (Group::Display, "ForwardConsoleCursorOnly") => self.forward_console_cursor_only = bool_value()?,
            _ => return Err(format!("Unknown property {}", property.full_name())),
        }
        let shown = match (property.kind, raw) {
            (ValueKind::Bool, None) => "True".to_string(),
            (_, Some(v)) => v,
            (_, None) => String::new(),
        };
        self.effective_args.push(format!("{}={}", property.full_name(), shown));
        Ok(())
    }
}
