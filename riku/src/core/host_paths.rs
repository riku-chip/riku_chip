//! Rutas de Windows dentro del contenedor (o de WSL).
//!
//! En una laptop con Windows, Riku corre en Linux: dentro de un contenedor
//! (iic-osic-tools) o de WSL. De los discos de Windows solo ve las carpetas
//! montadas (`C:\Users\…\designs` → `/foss/designs`, `C:\` → `/mnt/c`), y
//! qué carpeta de Windows es cada montaje lo dice `/proc/self/mountinfo`
//! (montajes `9p`/`drvfs` con `path=C:\`). Con eso una ruta de Windows
//! pegada en el visor se traduce a la del contenedor, y al revés.

use std::path::{Path, PathBuf};

/// Una carpeta de Windows montada.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostMount {
    /// Como la ve Windows: `C:\Users\Usuario\eda\designs`.
    pub windows: String,
    /// Dónde está montada: `/foss/designs`.
    pub local: PathBuf,
}

/// Las carpetas de Windows montadas en este sistema (vacío fuera de Linux,
/// o si no hay ninguna).
pub fn host_mounts() -> Vec<HostMount> {
    std::fs::read_to_string("/proc/self/mountinfo").map(|t| parse_mountinfo(&t)).unwrap_or_default()
}

/// Montajes de `mountinfo` que vienen de una unidad de Windows. Cada línea:
/// `id padre dev raíz punto opciones… - tipo origen superopciones`.
pub fn parse_mountinfo(text: &str) -> Vec<HostMount> {
    let mut out: Vec<HostMount> = text
        .lines()
        .filter_map(|line| {
            let (pre, post) = line.split_once(" - ")?;
            let pre: Vec<&str> = pre.split(' ').collect();
            let (root, point) = (unescape(pre.get(3)?), unescape(pre.get(4)?));
            let mut post = post.split(' ');
            let (_fstype, source, options) = (post.next()?, unescape(post.next()?), post.next().unwrap_or(""));
            // La unidad: `path=C:\` en las superopciones (drvfs/9p), o el origen.
            let drive = options
                .split([';', ','])
                .find_map(|o| o.strip_prefix("path="))
                .map(unescape)
                .filter(|p| drive_of(p).is_some())
                .or_else(|| drive_of(&source).map(|_| source.clone()))?;
            let base = drive.trim_end_matches(['\\', '/']);
            let rest = root.trim_end_matches('/').replace('/', "\\");
            Some(HostMount { windows: format!("{base}{rest}"), local: PathBuf::from(point) })
        })
        .collect();
    // La misma carpeta montada dos veces: una alcanza.
    out.dedup_by(|a, b| a.windows.eq_ignore_ascii_case(&b.windows) && a.local == b.local);
    out
}

/// `\134` → `\` (mountinfo escapa espacios, tabs y barras en octal).
fn unescape(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let octal = bytes.get(i + 1..i + 4).filter(|d| d.iter().all(|c| (b'0'..=b'7').contains(c)));
        match (bytes[i], octal) {
            (b'\\', Some(d)) => {
                out.push((d[0] - b'0') * 64 + (d[1] - b'0') * 8 + (d[2] - b'0'));
                i += 4;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// La letra de unidad de una ruta de Windows (`C:\…`, `c:/…`).
fn drive_of(s: &str) -> Option<char> {
    let mut chars = s.chars();
    let (letter, colon) = (chars.next()?, chars.next()?);
    let after = chars.next();
    (letter.is_ascii_alphabetic() && colon == ':' && matches!(after, None | Some('\\') | Some('/')))
        .then(|| letter.to_ascii_uppercase())
}

/// `true` si parece una ruta de Windows con unidad (`C:\…`).
pub fn is_windows_path(s: &str) -> bool {
    drive_of(s.trim()).is_some()
}

/// `C:\a\b\` → `c:/a/b`, para comparar sin importar barras ni mayúsculas
/// (Windows no distingue mayúsculas en las rutas).
fn normalized(s: &str) -> String {
    // ASCII: así el largo no cambia y el resto se recorta del original.
    s.trim().replace('\\', "/").trim_end_matches('/').to_ascii_lowercase()
}

/// La ruta local de una ruta de Windows, si cae en una carpeta montada (la
/// más específica). Acepta `\` o `/`, y comillas alrededor (como las copia
/// el Explorador con "Copiar como ruta de acceso").
pub fn to_local(windows: &str, mounts: &[HostMount]) -> Option<PathBuf> {
    let trimmed = windows.trim().trim_matches('"');
    let target = normalized(trimmed);
    let original = trimmed.replace('\\', "/").trim_end_matches('/').to_string();
    mounts
        .iter()
        .filter_map(|m| {
            let base = normalized(&m.windows);
            let rest = target.strip_prefix(&base)?;
            if !(rest.is_empty() || rest.starts_with('/')) {
                return None;
            }
            // El resto con sus mayúsculas originales (Linux sí las distingue).
            let rest = original.get(original.len() - rest.len()..).unwrap_or(rest).trim_matches('/');
            Some((base.len(), if rest.is_empty() { m.local.clone() } else { m.local.join(rest) }))
        })
        .max_by_key(|(len, _)| *len)
        .map(|(_, p)| p)
}

/// Cómo se llama `local` en Windows, si está en una carpeta montada.
pub fn to_windows(local: &Path, mounts: &[HostMount]) -> Option<String> {
    mounts
        .iter()
        .filter_map(|m| {
            let rest = local.strip_prefix(&m.local).ok()?;
            let parts: Vec<String> = rest.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
            let windows = if parts.is_empty() { m.windows.clone() } else { format!("{}\\{}", m.windows, parts.join("\\")) };
            Some((m.local.as_os_str().len(), windows))
        })
        .max_by_key(|(len, _)| *len)
        .map(|(_, w)| w)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Líneas reales de un contenedor de iic-osic-tools en Docker Desktop
    /// (WSL2) y de WSL con `/mnt/c`.
    const MOUNTINFO: &str = "\
1819 1810 0:99 /Users/Usuario/eda/designs /foss/designs rw,noatime - 9p C:\\134 rw,aname=drvfs;path=C:\\;uid=0;gid=0;metadata;symlinkroot=/mnt/host/,cache=0x5
1820 1810 0:71 / /mnt/wslg rw,relatime - tmpfs none rw
97 30 0:50 / /mnt/c rw,noatime - 9p drvfs rw,dirsync,aname=drvfs;path=C:\\;uid=1000;gid=1000
98 30 0:51 / /mnt/d\\040disco rw,noatime - 9p drvfs rw,aname=drvfs;path=D:\\;uid=1000
";

    fn mounts() -> Vec<HostMount> {
        parse_mountinfo(MOUNTINFO)
    }

    #[test]
    fn lee_las_carpetas_de_windows_montadas() {
        let m = mounts();
        assert_eq!(
            m,
            vec![
                HostMount { windows: r"C:\Users\Usuario\eda\designs".into(), local: "/foss/designs".into() },
                HostMount { windows: "C:".into(), local: "/mnt/c".into() },
                HostMount { windows: "D:".into(), local: "/mnt/d disco".into() },
            ]
        );
    }

    #[test]
    fn traduce_de_windows_al_contenedor() {
        let m = mounts();
        // La más específica gana sobre `/mnt/c`.
        assert_eq!(to_local(r"C:\Users\Usuario\eda\designs\riku_chip", &m), Some("/foss/designs/riku_chip".into()));
        assert_eq!(to_local(r#""c:/users/usuario/eda/designs/Riku""#, &m), Some("/foss/designs/Riku".into()));
        assert_eq!(to_local(r"C:\Users\Usuario\eda\designs\", &m), Some("/foss/designs".into()));
        assert_eq!(to_local(r"C:\Users\Usuario\eda\designs\Diseño\", &m), Some("/foss/designs/Diseño".into()));
        assert_eq!(to_local(r"C:\Windows", &m), Some("/mnt/c/Windows".into()));
        assert_eq!(to_local(r"E:\otra", &m), None, "unidad sin montar");
        assert_eq!(to_local(r"C:\Users\Usuario\eda\designsX", &m), Some("/mnt/c/Users/Usuario/eda/designsX".into()));
        let solo_designs = &m[..1];
        assert_eq!(to_local(r"C:\Users\Usuario\Desktop", solo_designs), None, "fuera de lo compartido");
    }

    #[test]
    fn y_del_contenedor_a_windows() {
        let m = mounts();
        assert_eq!(to_windows(Path::new("/foss/designs/riku_chip/docs"), &m).as_deref(), Some(r"C:\Users\Usuario\eda\designs\riku_chip\docs"));
        assert_eq!(to_windows(Path::new("/foss/designs"), &m).as_deref(), Some(r"C:\Users\Usuario\eda\designs"));
        assert_eq!(to_windows(Path::new("/headless"), &m), None);
    }

    #[test]
    fn reconoce_rutas_de_windows() {
        assert!(is_windows_path(r"C:\Users") && is_windows_path("d:/x") && is_windows_path("C:"));
        assert!(!is_windows_path("/foss/designs") && !is_windows_path("C:x") && !is_windows_path("abc"));
    }
}
