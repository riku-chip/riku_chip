//! Color en la salida de texto. Por defecto (`--color auto`): solo si stdout es una
//! terminal y no hay `NO_COLOR`, o si `CLICOLOR_FORCE=1` lo fuerza (como el grafo de
//! `log`). Redirigida a un archivo o a otro programa, la salida queda sin códigos.
//! `--color always|never` manda sobre el entorno y la terminal.

use std::io::IsTerminal;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

/// Cuándo poner color (`--color`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum ColorMode {
    /// Terminal, sin `NO_COLOR`; o `CLICOLOR_FORCE=1`.
    #[default]
    Auto,
    Always,
    Never,
}

/// `Always` y `Never` mandan; `Auto` es la regla de siempre.
pub fn resolve(mode: ColorMode, is_tty: bool, clicolor_force: bool, no_color: bool) -> bool {
    match mode {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => clicolor_force || (is_tty && !no_color),
    }
}

static MODE: AtomicU8 = AtomicU8::new(ColorMode::Auto as u8);

/// Fija el modo de este comando. Quien lo fije debe volver a `Auto` al terminar: en el
/// shell interactivo el proceso sigue vivo y el modo no debe pasar al comando siguiente.
pub fn set_mode(mode: ColorMode) {
    MODE.store(mode as u8, Ordering::Relaxed);
}

fn mode() -> ColorMode {
    match MODE.load(Ordering::Relaxed) {
        1 => ColorMode::Always,
        2 => ColorMode::Never,
        _ => ColorMode::Auto,
    }
}

/// ¿Va con color la salida de ahora? Lo del entorno se lee una vez.
pub fn enabled() -> bool {
    static ENV: OnceLock<(bool, bool, bool)> = OnceLock::new();
    let (is_tty, force, no_color) = *ENV.get_or_init(|| {
        let env = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty() && v != "0");
        (std::io::stdout().is_terminal(), env("CLICOLOR_FORCE"), env("NO_COLOR"))
    });
    resolve(mode(), is_tty, force, no_color)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_follows_terminal_and_environment() {
        let auto = |tty, force, no| resolve(ColorMode::Auto, tty, force, no);
        assert!(auto(true, false, false), "terminal");
        assert!(!auto(false, false, false), "redirigida");
        assert!(!auto(true, false, true), "NO_COLOR");
        assert!(auto(false, true, false), "CLICOLOR_FORCE sin terminal");
        assert!(auto(true, true, true), "CLICOLOR_FORCE gana a NO_COLOR");
    }

    #[test]
    fn always_and_never_beat_terminal_and_environment() {
        for tty in [false, true] {
            for force in [false, true] {
                for no in [false, true] {
                    assert!(resolve(ColorMode::Always, tty, force, no), "always: tty={tty} force={force} no={no}");
                    assert!(!resolve(ColorMode::Never, tty, force, no), "never: tty={tty} force={force} no={no}");
                }
            }
        }
    }

    #[test]
    fn mode_round_trips() {
        for m in [ColorMode::Always, ColorMode::Never, ColorMode::Auto] {
            set_mode(m);
            assert_eq!(mode(), m);
        }
    }
}
