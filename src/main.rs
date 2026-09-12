fn main() {
    // Die quietly when stdout is closed early (e.g. piped into `head`) instead of panicking.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = avdumpr::ui::app::run(args);
    std::process::exit(code);
}
