fn main() -> std::process::ExitCode {
    // Como git: si quien lee la salida la corta (`riku log | head`), terminar
    // en silencio en vez de fallar con "Broken pipe".
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    riku::cli::run()
}
