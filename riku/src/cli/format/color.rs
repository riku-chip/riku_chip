//! Color en la salida de texto: solo si stdout es una terminal y no hay
//! `NO_COLOR`, o si `CLICOLOR_FORCE=1` lo fuerza (como el grafo de `log`).
//! Redirigida a un archivo o a otro programa, la salida queda sin códigos.

use std::io::IsTerminal;
use std::sync::OnceLock;

fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        let env = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty() && v != "0");
        env("CLICOLOR_FORCE") || (std::io::stdout().is_terminal() && !env("NO_COLOR"))
    })
}

fn paint(s: &str, code: &str) -> String {
    if enabled() {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub fn green(s: &str) -> String {
    paint(s, "32")
}

pub fn red(s: &str) -> String {
    paint(s, "31")
}

pub fn yellow(s: &str) -> String {
    paint(s, "33")
}

pub fn bold(s: &str) -> String {
    paint(s, "1")
}

/// Marcador de cambio con su color: `+` verde, `-` rojo, `~`/`r` amarillo.
pub fn marker(m: &str) -> String {
    match m {
        "+" => green(m),
        "-" | "!" => red(m),
        _ => yellow(m),
    }
}
