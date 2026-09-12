//! Help output: a coloured, aligned overview of all namespaces and arguments.

use super::{all_properties, Group, SettingProperty};
use std::fmt::Write as _;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// ANSI styling that switches itself off for non-terminals / `NO_COLOR`.
#[derive(Clone, Copy)]
pub struct Style {
    enabled: bool,
}

impl Style {
    pub fn auto() -> Self {
        use std::io::IsTerminal;
        let enabled = std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal();
        Self { enabled }
    }
    pub fn plain() -> Self {
        Self { enabled: false }
    }
    fn wrap(&self, code: &str, s: &str) -> String {
        if self.enabled { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() }
    }
    pub fn bold(&self, s: &str) -> String {
        self.wrap("1", s)
    }
    pub fn dim(&self, s: &str) -> String {
        self.wrap("2", s)
    }
    pub fn title(&self, s: &str) -> String {
        self.wrap("1;36", s)
    }
    pub fn namespace(&self, s: &str) -> String {
        self.wrap("1;35", s)
    }
    pub fn arg(&self, s: &str) -> String {
        self.wrap("1;32", s)
    }
    pub fn example(&self, s: &str) -> String {
        self.wrap("36", s)
    }
    pub fn default(&self, s: &str) -> String {
        self.wrap("33", s)
    }
    pub fn muted(&self, s: &str) -> String {
        self.wrap("90", s)
    }
}

fn terminal_width() -> usize {
    crossterm::terminal::size().map(|(w, _)| w as usize).unwrap_or(100).clamp(60, 140)
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut cur = String::new();
        for word in para.split_whitespace() {
            if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > width {
                lines.push(std::mem::take(&mut cur));
            }
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(word);
        }
        lines.push(cur);
    }
    lines
}

fn default_display(style: &Style, p: &SettingProperty) -> String {
    match &p.default_display {
        None => style.muted("<null>"),
        Some(s) if s.is_empty() => style.muted("<Empty>"),
        Some(s) => style.default(s),
    }
}

/// Render the help text. `topic` restricts the output to one namespace, `detailed` adds the
/// descriptions (the original shows them whenever `--Help` was given explicitly).
pub fn render(style: &Style, topic: &str, detailed: bool) -> String {
    let properties = all_properties();
    let width = terminal_width();
    let mut out = String::new();

    let groups: Vec<Group> = Group::ALL.iter().copied().filter(|g| topic.is_empty() || g.name().eq_ignore_ascii_case(topic)).collect();
    if groups.is_empty() {
        out.push_str("There is no such topic\n\n");
        return out;
    }

    if topic.is_empty() {
        let banner = format!("AVDump3 {VERSION}");
        let tagline = "multi-hash & media metadata dumper";
        let inner = banner.chars().count() + tagline.chars().count() + 5;
        let _ = writeln!(out, "{}", style.title(&format!("╭{}╮", "─".repeat(inner))));
        let _ = writeln!(out, "{}  {} {} {}  {}", style.title("│"), style.bold(&banner), style.muted("·"), tagline, style.title("│"));
        let _ = writeln!(out, "{}", style.title(&format!("╰{}╯", "─".repeat(inner))));
        out.push('\n');
        let _ = writeln!(out, "{}", style.title("USAGE"));
        let _ = writeln!(out, "  avdumpr [--Option[=Value]]... <file or directory>...");
        let _ = writeln!(out, "  avdumpr FROMFILE <arguments.txt> [--Option...]");
        let _ = writeln!(out, "  avdumpr --Help[=<NameSpace>]");
        out.push('\n');
        let _ = writeln!(out, "{}", style.title("EXAMPLES"));
        let examples = [
            ("avdumpr --Consumers=ED2K,CRC32 --PrintHashes video.mkv", "hash a single file and print the digests"),
            ("avdumpr -R --Cons=ED2K,MKV --Reports=AVD3 --RDir=out /media", "recurse, parse Matroska structure, write XML reports"),
            ("avdumpr --Consumers", "list the available consumers"),
            ("avdumpr --NullStreamTest=4:1024:2 --Cons=SHA1,TTH", "benchmark hashing speed without disk I/O"),
        ];
        let ex_pad = examples.iter().map(|(c, _)| c.chars().count()).max().unwrap_or(0);
        for (cmd, what) in examples {
            let _ = writeln!(out, "  {}{}  {}", style.example(cmd), " ".repeat(ex_pad - cmd.chars().count()), style.muted(what));
        }
        out.push('\n');
        let _ = writeln!(out, "{}", style.title("ARGUMENT SYNTAX"));
        let _ = writeln!(out, "  --Name=Value  --NameSpace.Name=Value  --Switch  -X  -RXY (several single-letter switches)");
        let _ = writeln!(out, "  Names are case-insensitive; single-letter aliases are case-sensitive.");
        out.push('\n');
    }

    for group in groups {
        let props: Vec<&SettingProperty> = properties.iter().filter(|p| p.group == group).collect();
        let name_pad = props.iter().map(|p| p.names_display().chars().count()).max().unwrap_or(0).max(("NameSpace: ".len() + group.name().len()).min(30));

        let _ = writeln!(out, "{} {}  {}", style.namespace("▶"), style.namespace(&format!("NameSpace: {}", group.name())), style.muted(group.description()));
        let _ = writeln!(out, "{}", style.muted(&"─".repeat(width.min(100))));

        for p in props {
            let names = p.names_display();
            let pad = " ".repeat(name_pad.saturating_sub(names.chars().count()));
            let _ = writeln!(out, "  {}{}  {}  ({})", style.arg(&names), pad, style.example(p.example), default_display(style, p));
            if detailed && !p.description.is_empty() {
                let indent = "      ";
                for line in wrap_text(p.description, width.saturating_sub(indent.len() + 2)) {
                    let _ = writeln!(out, "{indent}{line}");
                }
                out.push('\n');
            }
        }
        if !detailed && topic.is_empty() {
            let _ = writeln!(out, "  {}", style.muted("Use --Help OR --Help=<NameSpace> for more detailed info"));
        }
        out.push('\n');
    }
    out
}

/// Markdown table of all arguments (`CLSettingsHandler.PrintHelpMarkdown`).
pub fn render_markdown() -> String {
    let mut out = String::new();
    out.push_str("|Argument|Namespace|Description|Default|Example\n|--|--|--|--|--\n");
    let esc = |s: &str| s.replace('<', "\\<").replace('|', "\\|").replace('\r', "").replace('\n', "<br>");
    for p in all_properties() {
        let default = p.default_display.clone().unwrap_or_default();
        let _ = writeln!(out, "|{}|{}|{}|{}|{}", esc(&p.names_display()), p.group.name(), esc(p.description), esc(&default), esc(p.example));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_lists_every_argument() {
        let text = render(&Style::plain(), "", true);
        for p in all_properties() {
            assert!(text.contains(&format!("--{}", p.name)), "missing {}", p.name);
            let first_words: String = p.description.split_whitespace().take(3).collect::<Vec<_>>().join(" ");
            assert!(text.contains(&first_words), "missing description of {}", p.name);
        }
        assert!(text.contains("NameSpace: Display"));
    }

    #[test]
    fn help_topic_filters() {
        let text = render(&Style::plain(), "Reporting", true);
        assert!(text.contains("--PrintHashes"));
        assert!(!text.contains("--Recursive"));
        assert!(render(&Style::plain(), "Bogus", true).contains("There is no such topic"));
    }

    #[test]
    fn markdown_has_rows() {
        let md = render_markdown();
        assert!(md.lines().count() > 40);
        assert!(md.contains("|--Recursive, -R|FileDiscovery|"));
    }
}
