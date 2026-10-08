//! The banner: the seal of the National University of Engineering (UNI,
//! Lima, Peru) and the credits, when the shell starts and in `riku about`.
//!
//! The best the terminal can draw:
//! - **Sixel** (Windows Terminal 1.22+, WezTerm, foot, Konsole, mlterm, xterm
//!   with `-ti vt340`…): the real image, antialiased with 16 shades between
//!   white and maroon, on white inside the oval;
//! - **braille** in maroon, in any terminal with colors;
//! - **text** only (no colors, `TERM=dumb`, a narrow window).
//!
//! `RIKU_BANNER=sixel|braille|text|off` forces one. Never when the output is
//! not a terminal (scripts, CI, pipes).

use std::io::{IsTerminal, Write};

use crate::i18n::tr;

/// The seal, 480 px wide: only the alpha channel matters (one color).
const LOGO: &[u8] = include_bytes!("../../assets/uni-logo.png");
const MAROON: (u8, u8, u8) = (128, 0, 0);

/// The wordmark, for terminals without colors.
const WORDMARK: &str = r#"
    ██████╗ ██╗██╗  ██╗██╗   ██╗
    ██╔══██╗██║██║ ██╔╝██║   ██║
    ██████╔╝██║█████╔╝ ██║   ██║
    ██╔══██╗██║██╔═██╗ ██║   ██║
    ██║  ██║██║██║  ██╗╚██████╔╝
    ╚═╝  ╚═╝╚═╝╚═╝  ╚═╝ ╚═════╝
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Sixel,
    Braille,
    Text,
    Off,
}

/// The credits, one line each; `true`: in bold.
fn credits() -> Vec<(String, bool)> {
    vec![
        (format!("Riku {}", env!("CARGO_PKG_VERSION")), true),
        (tr!("about.tagline").to_string(), false),
        (String::new(), false),
        (tr!("about.university").to_string(), true),
        (tr!("about.place").to_string(), false),
        (String::new(), false),
        (tr!("about.authors").to_string(), false),
        (format!("  Carlos Cueva · {}", tr!("about.engineer")), false),
        (format!("  Amado Frias  · {}", tr!("about.engineer")), false),
    ]
}

/// The seal's alpha (0..1), `w × h`.
fn mask() -> Option<(usize, usize, Vec<f32>)> {
    let pix = resvg::tiny_skia::Pixmap::decode_png(LOGO).ok()?;
    let (w, h) = (pix.width() as usize, pix.height() as usize);
    let alpha = pix.pixels().iter().map(|p| p.alpha() as f32 / 255.0).collect();
    Some((w, h, alpha))
}

/// The mask at `ow × oh`: each output pixel is the average of the source
/// pixels it covers (smooth edges when shrinking).
fn resample((w, h, a): &(usize, usize, Vec<f32>), ow: usize, oh: usize) -> Vec<f32> {
    let mut out = vec![0.0; ow * oh];
    for oy in 0..oh {
        let (y0, y1) = (oy * h / oh, ((oy + 1) * h / oh).max(oy * h / oh + 1).min(*h));
        for ox in 0..ow {
            let (x0, x1) = (ox * w / ow, ((ox + 1) * w / ow).max(ox * w / ow + 1).min(*w));
            let mut sum = 0.0;
            for y in y0..y1 {
                sum += a[y * w + x0..y * w + x1].iter().sum::<f32>();
            }
            out[oy * ow + ox] = sum / ((y1 - y0) * (x1 - x0)) as f32;
        }
    }
    out
}

/// The seal in braille: 2 × 4 dots per character, `cols` wide.
fn braille(cols: usize) -> Vec<String> {
    let Some(m) = mask() else { return Vec::new() };
    let (w, h) = (cols * 2, (cols * 2 * m.1 / m.0).div_ceil(4) * 4);
    let a = resample(&m, w, h);
    const BITS: [(usize, usize, u32); 8] =
        [(0, 0, 0x01), (0, 1, 0x02), (0, 2, 0x04), (1, 0, 0x08), (1, 1, 0x10), (1, 2, 0x20), (0, 3, 0x40), (1, 3, 0x80)];
    (0..h / 4)
        .map(|r| {
            (0..cols)
                .map(|c| {
                    let v = BITS.iter().filter(|(dx, dy, _)| a[(r * 4 + dy) * w + c * 2 + dx] > 0.47).map(|b| b.2).sum::<u32>();
                    if v == 0 {
                        ' '
                    } else {
                        char::from_u32(0x2800 + v).unwrap_or(' ')
                    }
                })
                .collect()
        })
        .collect()
}

/// The seal as Sixel, `width` px: on white inside the oval, 16 shades
/// between white and maroon; transparent outside.
fn sixel(width: usize) -> String {
    const SHADES: usize = 16;
    let Some(m) = mask() else { return String::new() };
    let height = (width * m.1 / m.0) / 6 * 6;
    let a = resample(&m, width, height);
    // 0: transparent; 1..=SHADES: white … maroon.
    let mut idx = vec![0u8; width * height];
    for y in 0..height {
        for x in 0..width {
            let (dx, dy) = ((x as f32 + 0.5) / width as f32 * 2.0 - 1.0, (y as f32 + 0.5) / height as f32 * 2.0 - 1.0);
            let r = (dx * dx + dy * dy).sqrt();
            let alpha = a[y * width + x];
            idx[y * width + x] = if r <= 0.985 {
                1 + (alpha * (SHADES - 1) as f32).round() as u8
            } else if alpha > 0.5 {
                SHADES as u8
            } else {
                0
            };
        }
    }
    let pct = |v: f32| (v * 100.0 / 255.0).round() as u32;
    let mut out = format!("\x1bP0;1;0q\"1;1;{width};{height}");
    for k in 1..=SHADES {
        let t = (k - 1) as f32 / (SHADES - 1) as f32;
        let mix = |c: u8| 255.0 * (1.0 - t) + c as f32 * t;
        out += &format!("#{k};2;{};{};{}", pct(mix(MAROON.0)), pct(mix(MAROON.1)), pct(mix(MAROON.2)));
    }
    for band in (0..height).step_by(6) {
        let mut used: Vec<u8> =
            (0..6).flat_map(|dy| (0..width).map(move |x| (dy, x))).map(|(dy, x)| idx[(band + dy) * width + x]).collect();
        used.sort_unstable();
        used.dedup();
        let rows: Vec<String> = used
            .into_iter()
            .filter(|&k| k != 0)
            .map(|k| {
                let row: String = (0..width)
                    .map(|x| {
                        let bits = (0..6).filter(|&dy| idx[(band + dy) * width + x] == k).map(|dy| 1u8 << dy).sum::<u8>();
                        (63 + bits) as char
                    })
                    .collect();
                format!("#{k}{}", row.trim_end_matches('?'))
            })
            .collect();
        out += &rows.join("$");
        out.push('-');
    }
    out += "\x1b\\";
    out
}

/// Which banner this terminal can show.
pub(crate) fn detect() -> Mode {
    match std::env::var("RIKU_BANNER").ok().as_deref() {
        Some("off") => return Mode::Off,
        Some("text") => return Mode::Text,
        Some("braille") => return Mode::Braille,
        Some("sixel") => return Mode::Sixel,
        _ => {}
    }
    if !std::io::stdout().is_terminal() {
        return Mode::Off;
    }
    let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    if no_color || std::env::var("TERM").is_ok_and(|t| t == "dumb") || term_cols() < 80 {
        return Mode::Text;
    }
    if supports_sixel() {
        Mode::Sixel
    } else {
        Mode::Braille
    }
}

/// Columns of the terminal (80 if unknown).
fn term_cols() -> usize {
    #[cfg(unix)]
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 {
            return ws.ws_col as usize;
        }
    }
    std::env::var("COLUMNS").ok().and_then(|c| c.parse().ok()).unwrap_or(if std::env::var_os("WT_SESSION").is_some() {
        120
    } else {
        80
    })
}

/// If the terminal draws Sixel. On Unix, it is asked (Primary Device
/// Attributes: a `4` in the answer); on Windows, Windows Terminal and WezTerm
/// do.
fn supports_sixel() -> bool {
    if std::env::var("TERM_PROGRAM").is_ok_and(|p| p == "WezTerm") {
        return true;
    }
    #[cfg(unix)]
    {
        da1().is_some_and(|attrs| attrs.split(';').any(|a| a == "4"))
    }
    #[cfg(not(unix))]
    {
        std::env::var_os("WT_SESSION").is_some()
    }
}

/// The terminal's Primary Device Attributes (`ESC [ ? 62;4;22 c` → `62;4;22`),
/// asked on `/dev/tty` with a 200 ms limit.
#[cfg(unix)]
fn da1() -> Option<String> {
    use std::os::fd::AsRawFd;
    let mut tty = std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty").ok()?;
    let fd = tty.as_raw_fd();
    unsafe {
        let mut old: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut old) != 0 {
            return None;
        }
        let mut raw = old;
        raw.c_lflag &= !(libc::ICANON | libc::ECHO);
        raw.c_cc[libc::VMIN] = 0;
        raw.c_cc[libc::VTIME] = 0;
        libc::tcsetattr(fd, libc::TCSANOW, &raw);
        let _ = tty.write_all(b"\x1b[c");
        let _ = tty.flush();
        let mut answer = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(200);
        while std::time::Instant::now() < deadline && !answer.ends_with(b"c") {
            let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
            if libc::poll(&mut pfd, 1, 20) > 0 {
                let mut buf = [0u8; 64];
                let n = libc::read(fd, buf.as_mut_ptr().cast(), buf.len());
                if n > 0 {
                    answer.extend_from_slice(&buf[..n as usize]);
                }
            }
        }
        libc::tcsetattr(fd, libc::TCSANOW, &old);
        let text = String::from_utf8_lossy(&answer).to_string();
        let start = text.find("\x1b[?")? + 3;
        let end = text[start..].find('c')? + start;
        Some(text[start..end].to_string())
    }
}

/// Prints the banner in `mode`.
pub(crate) fn print(mode: Mode) {
    let credits = credits();
    let mut out = std::io::stdout().lock();
    match mode {
        Mode::Off => {}
        Mode::Text => {
            let _ = write!(out, "{WORDMARK}");
            for (line, _) in &credits {
                let _ = writeln!(out, "    {line}");
            }
            let _ = writeln!(out);
        }
        Mode::Braille => {
            let color = std::env::var_os("NO_COLOR").is_none();
            let logo = braille(if term_cols() >= 110 { 46 } else { 34 });
            let pad = logo.len().saturating_sub(credits.len()) / 2;
            let _ = writeln!(out);
            for (i, line) in logo.iter().enumerate() {
                let (text, bold) = i.checked_sub(pad).and_then(|j| credits.get(j)).cloned().unwrap_or_default();
                let (c0, b0, end) =
                    if color { ("\x1b[38;2;128;0;0m", if bold { "\x1b[1m" } else { "" }, "\x1b[0m") } else { ("", "", "") };
                let _ = writeln!(out, "  {c0}{line}{end}    {b0}{text}{end}");
            }
            let _ = writeln!(out);
        }
        Mode::Sixel => {
            // Room first (the terminal scrolls now, not in the middle), then
            // the credits to the right (at a column past the image even with
            // narrow cells), and the image over the left part.
            const WIDTH: usize = 220;
            const ROWS: usize = 16;
            let _ = write!(out, "\n{}", "\n".repeat(ROWS));
            let _ = write!(out, "\x1b[{ROWS}A\r\x1b7");
            let col = WIDTH / 8 + 6;
            let pad = (ROWS / 2).saturating_sub(credits.len() / 2 + 1);
            for (i, (text, bold)) in credits.iter().enumerate() {
                let b = if *bold { "\x1b[1m" } else { "" };
                let down = if pad + i > 0 { format!("\x1b[{}B", pad + i) } else { String::new() };
                let _ = write!(out, "\x1b8{down}\x1b[{col}C{b}{text}\x1b[0m");
            }
            let _ = write!(out, "\x1b8  {}", sixel(WIDTH));
            let _ = writeln!(out);
        }
    }
    let _ = out.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seal_is_drawn_in_braille_and_sixel() {
        let lines = braille(40);
        assert!(lines.len() > 20 && lines.len() < 30, "{} filas", lines.len());
        assert!(lines.iter().all(|l| l.chars().count() == 40));
        assert!(lines.iter().any(|l| l.contains('⣿')), "el busto y el engranaje son sólidos");
        let s = sixel(120);
        assert!(s.starts_with("\x1bP0;1;0q") && s.ends_with("\x1b\\"));
        assert!(s.contains("#16;2;50;0;0"), "el granate");
    }

    #[test]
    fn the_credits_carry_the_version() {
        let c = credits();
        assert_eq!(c[0].0, format!("Riku {}", env!("CARGO_PKG_VERSION")));
        assert!(c.iter().any(|(l, _)| l.contains("Carlos Cueva")) && c.iter().any(|(l, _)| l.contains("Amado Frias")));
    }
}
