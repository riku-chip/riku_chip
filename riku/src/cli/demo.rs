//! `riku demo`: proyectos de ejemplo con historia, para probar Riku sin un
//! diseño propio. Cada uno es un repo Git empaquetado (`git bundle`) dentro
//! del ejecutable; se generan con `tools/demos/` (ver `docs/dev/development.md`).

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::i18n::tr;

/// Un proyecto de ejemplo: nombre, clave de su descripción y el bundle.
struct Demo {
    name: &'static str,
    about: &'static str,
    bundle: &'static [u8],
}

const DEMOS: &[Demo] = &[
    Demo { name: "ota", about: "demo.about.ota", bundle: include_bytes!("../../../examples/demos/ota.bundle") },
    Demo { name: "sram", about: "demo.about.sram", bundle: include_bytes!("../../../examples/demos/sram.bundle") },
    Demo { name: "inversor", about: "demo.about.inversor", bundle: include_bytes!("../../../examples/demos/inversor.bundle") },
    Demo { name: "chip", about: "demo.about.chip", bundle: include_bytes!("../../../examples/demos/chip.bundle") },
];

/// `riku demo [nombre] [--dir DIR] [--list]`.
pub(crate) fn run(name: Option<String>, dir: Option<PathBuf>, list: bool) -> Result<(), String> {
    if list {
        for d in DEMOS {
            println!("  {:<10} {}", d.name, tr!(d.about));
        }
        return Ok(());
    }
    let chosen: Vec<&Demo> = match &name {
        Some(n) => vec![DEMOS
            .iter()
            .find(|d| d.name == n)
            .ok_or_else(|| tr!("demo.unknown", name = n, names = DEMOS.iter().map(|d| d.name).collect::<Vec<_>>().join(", ")))?],
        None => DEMOS.iter().collect(),
    };
    let root = match dir {
        Some(d) => d,
        None => home().ok_or_else(|| tr!("demo.no_home"))?.join("riku-demos"),
    };
    std::fs::create_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    for d in chosen {
        let path = root.join(d.name);
        if path.exists() {
            println!("{}", tr!("demo.exists", path = path.display()));
            continue;
        }
        create(d, &path)?;
        println!("{}", tr!("demo.created", name = d.name, path = path.display()));
        println!("    {}", tr!(d.about));
    }
    println!();
    println!("{}", tr!("demo.next", path = root.display()));
    Ok(())
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)
}

/// Clona el bundle en `path` como un repo normal: con sus ramas locales y
/// sin el remoto temporal.
fn create(d: &Demo, path: &Path) -> Result<(), String> {
    let tmp = std::env::temp_dir().join(format!("riku-demo-{}-{}.bundle", d.name, std::process::id()));
    std::fs::write(&tmp, d.bundle).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let result = (|| {
        git(None, &["clone", "-q", &tmp.to_string_lossy(), &path.to_string_lossy()])?;
        let branches = git(Some(path), &["for-each-ref", "--format=%(refname:strip=3)", "refs/remotes/origin"])?;
        for b in branches.lines().filter(|b| !b.is_empty() && *b != "HEAD" && *b != "main") {
            git(Some(path), &["branch", "-q", b, &format!("origin/{b}")])?;
        }
        git(Some(path), &["remote", "remove", "origin"])?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&tmp);
    if result.is_err() {
        let _ = std::fs::remove_dir_all(path);
    }
    result
}

fn git(dir: Option<&Path>, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    let out = cmd.args(args).output().map_err(|e| tr!("demo.no_git", error = e))?;
    if !out.status.success() {
        return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_demo_clones_with_its_branches_and_no_remote() {
        let dir = std::env::temp_dir().join(format!("riku-demo-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        run(None, Some(dir.clone()), false).expect("demos");
        let ota = dir.join("ota");
        assert!(ota.join("layout/ota-5t.gds").exists() && ota.join("sim/ota-5t_tb.raw").exists());
        let branches = git(Some(&ota), &["branch", "--format=%(refname:short)"]).unwrap();
        assert!(branches.lines().any(|b| b == "narrow-input-pair"), "{branches}");
        assert!(git(Some(&ota), &["remote"]).unwrap().trim().is_empty(), "sin el remoto temporal");
        assert!(git(Some(&ota), &["tag"]).unwrap().contains("v1.0"));
        assert!(dir.join("sram/sram_16x8_sky130.gds").exists());
        let inv = dir.join("inversor");
        assert!(inv.join("layout/inv.mag").exists() && inv.join("xschem/inv.sch").exists());
        let inv_branches = git(Some(&inv), &["branch", "--format=%(refname:short)"]).unwrap();
        assert!(inv_branches.lines().any(|b| b == "longer-nmos"), "{inv_branches}");
        assert!(dir.join("chip/sky130_sram_1kbyte_1rw1r_32x256_8.gds").exists());
        // Otra vez: no pisa lo que existe.
        run(Some("ota".into()), Some(dir.clone()), false).expect("otra vez");
        assert!(run(Some("nada".into()), Some(dir.clone()), false).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
