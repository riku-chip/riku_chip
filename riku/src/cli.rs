use std::path::PathBuf;
use std::process::{Command, ExitCode};

use clap::{Parser, Subcommand, ValueEnum};
use serde_json::json;

use crate::adapters::xschem_driver::XschemDriver;
use crate::core::diff_view::{summarize_changes, DiffView};
use crate::core::driver::RikuDriver;
use crate::core::git_service::GitService;
use crate::core::models::ChangeKind;
use crate::core::ports::GitRepository;
use crate::core::registry::get_drivers;

// ─── CLI types ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
    Visual,
}

#[derive(Parser, Debug)]
#[command(name = "riku", about = "Riku - VCS semantico para diseno de chips")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Muestra cambios semanticos entre dos commits para un archivo.
    Diff {
        commit_a: String,
        commit_b: String,
        file_path: String,
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,
        #[arg(short = 'f', long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Lista commits del branch actual, opcionalmente filtrados por archivo.
    Log {
        file_path: Option<String>,
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
        #[arg(short = 's', long)]
        semantic: bool,
    },
    /// Verifica que el entorno este correctamente configurado.
    Doctor {
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,
    },
    /// Renderiza un archivo .sch a SVG.
    Render { file: PathBuf },
    /// Abre la GUI de escritorio.
    Gui { file: Option<PathBuf> },
    /// Abre el shell interactivo de Riku.
    Shell {
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,
    },
}

// ─── Entry point ─────────────────────────────────────────────────────────────

pub fn run() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        None => run_shell(PathBuf::from(".")),
        Some(Commands::Diff { commit_a, commit_b, file_path, repo, format }) => {
            run_diff(repo, &commit_a, &commit_b, &file_path, format)
        }
        Some(Commands::Log { file_path, repo, limit, semantic }) => {
            run_log(repo, file_path.as_deref(), limit, semantic)
        }
        Some(Commands::Doctor { repo }) => run_doctor(repo),
        Some(Commands::Render { file }) => run_render(file),
        Some(Commands::Gui { file }) => run_gui(file),
        Some(Commands::Shell { repo }) => run_shell(repo),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}

// ─── Diff ────────────────────────────────────────────────────────────────────

fn run_diff(
    repo: PathBuf,
    commit_a: &str,
    commit_b: &str,
    file_path: &str,
    format: OutputFormat,
) -> Result<(), String> {
    let driver = XschemDriver::new();
    let view = DiffView::from_commits(&repo, commit_a, commit_b, file_path, &driver, |b| {
        crate::parsers::xschem::parse(b)
    })
    .map_err(|e| e.to_string())?;

    for w in &view.warnings {
        eprintln!("[!] {w}");
    }

    match format {
        OutputFormat::Text => present_text(&view, file_path),
        OutputFormat::Json => present_json(&view, file_path),
        OutputFormat::Visual => present_visual(&repo, commit_a, commit_b, file_path),
    }
}

fn present_text(view: &DiffView, file_path: &str) -> Result<(), String> {
    if view.report.is_empty() {
        println!("Sin cambios semanticos.");
        return Ok(());
    }

    let semantic: Vec<_> = view.report.components.iter().filter(|c| !c.cosmetic).collect();
    let cosmetic: Vec<_> = view.report.components.iter().filter(|c| c.cosmetic).collect();

    println!("Archivo : {file_path}");
    println!("Cambios : {}", semantic.len());
    if !cosmetic.is_empty() {
        println!("Cosméticos: {} (solo posición)", cosmetic.len());
    }
    println!();

    for c in &semantic {
        let is_rename = c.kind == ChangeKind::Modified && c.name.contains(" → ");
        let marker = if is_rename { "r" } else {
            match c.kind {
                ChangeKind::Added    => "+",
                ChangeKind::Removed  => "-",
                ChangeKind::Modified => "~",
            }
        };
        println!("  {marker} {}", c.name);

        // Parámetros que cambiaron
        if let (Some(before), Some(after)) = (&c.before, &c.after) {
            let all_keys: std::collections::BTreeSet<_> =
                before.keys().chain(after.keys()).collect();
            for key in all_keys {
                // Ignorar coordenadas en cambios semánticos — no aportan
                if matches!(key.as_str(), "x" | "y" | "rotation" | "mirror") {
                    continue;
                }
                match (before.get(key), after.get(key)) {
                    (Some(a), Some(b)) if a != b => {
                        println!("      {key}: {a} → {b}");
                    }
                    (None, Some(b)) => {
                        println!("      {key}: (nuevo) → {b}");
                    }
                    (Some(a), None) => {
                        println!("      {key}: {a} → (eliminado)");
                    }
                    _ => {}
                }
            }
        } else if c.kind == ChangeKind::Added {
            if let Some(after) = &c.after {
                if let Some(sym) = after.get("symbol") {
                    println!("      símbolo: {sym}");
                }
            }
        }
    }

    // Nets
    if !view.report.nets_added.is_empty() {
        println!();
        for net in &view.report.nets_added {
            println!("  + net:{net}");
        }
    }
    if !view.report.nets_removed.is_empty() {
        if view.report.nets_added.is_empty() { println!(); }
        for net in &view.report.nets_removed {
            println!("  - net:{net}");
        }
    }

    Ok(())
}

fn present_json(view: &DiffView, file_path: &str) -> Result<(), String> {
    let payload = json!({
        "file": file_path,
        "warnings": view.warnings,
        "components": view.report.components,
        "nets_added": view.report.nets_added,
        "nets_removed": view.report.nets_removed,
        "is_move_all": view.report.is_move_all,
    });
    println!("{}", serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?);
    Ok(())
}

fn present_visual(
    repo: &PathBuf,
    commit_a: &str,
    commit_b: &str,
    file_path: &str,
) -> Result<(), String> {
    let repo_abs = repo.canonicalize().unwrap_or_else(|_| repo.clone());
    let extra_args: Vec<std::ffi::OsString> = vec![
        "--repo".into(),
        repo_abs.into_os_string(),
        "--commit-a".into(),
        commit_a.into(),
        "--commit-b".into(),
        commit_b.into(),
        file_path.into(),
    ];

    run_gui_with_args(extra_args)
}

// ─── Render ──────────────────────────────────────────────────────────────────

fn run_render(file: PathBuf) -> Result<(), String> {
    let content = std::fs::read(&file).map_err(|e| e.to_string())?;
    let driver = XschemDriver::new();
    let svg_path = driver
        .render(&content, &file.to_string_lossy())
        .ok_or_else(|| "No se pudo renderizar el archivo.".to_string())?;
    println!("SVG: {}", svg_path.display());
    open_file(&svg_path)
}

// ─── Log ─────────────────────────────────────────────────────────────────────

fn run_log(
    repo: PathBuf,
    file_path: Option<&str>,
    limit: usize,
    semantic: bool,
) -> Result<(), String> {
    let svc = GitService::open(&repo).map_err(|e| e.to_string())?;
    let commits = GitRepository::get_commits(&svc, file_path).map_err(|e| e.to_string())?;

    if commits.is_empty() {
        println!("Sin commits encontrados.");
        return Ok(());
    }

    for (idx, commit) in commits.iter().take(limit).enumerate() {
        println!(
            "{}  {:<20}  {}",
            commit.short_id,
            commit.author,
            commit.message.chars().take(60).collect::<String>()
        );

        if semantic {
            if let Some(fp) = file_path {
                if idx + 1 < commits.len() {
                    if let Ok(report) = crate::core::analyzer::analyze_diff(
                        &repo,
                        &commits[idx + 1].oid,
                        &commit.oid,
                        fp,
                    ) {
                        let (added, removed, modified) = summarize_changes(&report.changes);
                        let mut parts = Vec::new();
                        if added > 0 { parts.push(format!("+{added}")); }
                        if removed > 0 { parts.push(format!("-{removed}")); }
                        if modified > 0 { parts.push(format!("~{modified}")); }
                        if !parts.is_empty() {
                            println!("           {}", parts.join("  "));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

// ─── Doctor ──────────────────────────────────────────────────────────────────

fn run_doctor(repo: PathBuf) -> Result<(), String> {
    println!("\nRiku Doctor — Diagnóstico del Entorno\n");
    let mut any_error = false;

    println!("--- Repositorio Git ---");
    match git2::Repository::discover(&repo) {
        Ok(r) => println!("  [ok]  {}", r.workdir().unwrap_or(r.path()).display()),
        Err(_) => println!("  [!]  No detectado — diff/log no funcionarán"),
    }

    println!("\n--- PDK ---");
    let pdk_root = std::env::var("PDK_ROOT").ok();
    let pdk_name = std::env::var("PDK").ok();
    let tools = std::env::var("TOOLS").ok();

    let xschemrc_local = std::path::PathBuf::from(".xschemrc");
    let xschemrc_home = dirs::home_dir().map(|h| h.join(".xschemrc"));
    let xschemrc = if xschemrc_local.exists() {
        Some(xschemrc_local)
    } else {
        xschemrc_home.filter(|p| p.exists())
    };

    match &xschemrc {
        Some(p) => println!("  [ok]  .xschemrc: {}", p.display()),
        None => println!("  [--]  .xschemrc: no encontrado"),
    }

    match (&pdk_root, &pdk_name) {
        (Some(root), Some(pdk)) => {
            let sym = std::path::Path::new(root).join(pdk).join("libs.tech/xschem");
            if sym.exists() {
                println!("  [ok]  $PDK_ROOT/$PDK → {}", sym.display());
            } else {
                println!("  [!]  $PDK_ROOT/$PDK configurado pero ruta no encontrada: {}", sym.display());
            }
        }
        _ => println!("  [--]  $PDK_ROOT / $PDK: no configurados"),
    }

    match &tools {
        Some(t) => {
            let devices = std::path::Path::new(t)
                .join("xschem/share/xschem/xschem_library/devices");
            if devices.exists() {
                println!("  [ok]  $TOOLS → {}", devices.display());
            } else {
                println!("  [!]  $TOOLS configurado pero devices no encontrado: {}", devices.display());
            }
        }
        None => println!("  [--]  $TOOLS: no configurado"),
    }

    let has_symbols = xschemrc.is_some()
        || pdk_root.as_ref().zip(pdk_name.as_ref()).map(|(r, p)| {
            std::path::Path::new(r).join(p).join("libs.tech/xschem").exists()
        }).unwrap_or(false)
        || tools.as_ref().map(|t| {
            std::path::Path::new(t).join("xschem/share/xschem/xschem_library/devices").exists()
        }).unwrap_or(false);

    if !has_symbols {
        println!("  [!]  Sin fuente de símbolos — los componentes se renderizarán como cajas vacías");
    }

    println!("\n--- Drivers ---");
    for driver in get_drivers() {
        let info = driver.info();
        let status = if info.available { "[ok]" } else { "[x]" };
        println!("  {status}  {:10} {}", info.name, info.version);
    }

    println!("\n--- Sistema ---");
    let cache = dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("riku");
    if std::fs::create_dir_all(&cache).is_ok() {
        println!("  [ok]  Caché: {}", cache.display());
    } else {
        println!("  [x]  Caché: no accesible en {}", cache.display());
        any_error = true;
    }

    println!();
    if any_error {
        return Err("Se detectaron problemas críticos en el entorno.".to_string());
    }
    println!("Entorno listo.\n");
    Ok(())
}

// ─── GUI ─────────────────────────────────────────────────────────────────────

fn run_gui(file: Option<PathBuf>) -> Result<(), String> {
    let args: Vec<std::ffi::OsString> =
        file.into_iter().map(|p| p.into_os_string()).collect();
    run_gui_with_args(args)
}

fn run_gui_with_args(args: Vec<std::ffi::OsString>) -> Result<(), String> {
    if let Some(bin) = locate_gui_binary() {
        let status = Command::new(bin).args(&args).status().map_err(|e| e.to_string())?;
        return if status.success() { Ok(()) } else {
            Err(format!("riku-gui finalizó con error: {status}"))
        };
    }

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| "No se pudo resolver la raíz del workspace.".to_string())?;

    let mut cargo = Command::new("cargo");
    cargo.args(["run", "--package", "riku-gui", "--bin", "riku-gui"])
        .current_dir(workspace_root);
    if !args.is_empty() {
        cargo.arg("--").args(&args);
    }

    let status = cargo.status().map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else {
        Err(format!("No se pudo iniciar riku-gui: {status}"))
    }
}

fn locate_gui_binary() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("RIKU_GUI_BIN") {
        let candidate = PathBuf::from(path);
        if candidate.exists() { return Some(candidate); }
    }
    let exe = std::env::current_exe().ok()?;
    let bin_name = format!("riku-gui{}", std::env::consts::EXE_SUFFIX);
    let sibling = exe.parent()?.join(bin_name);
    if sibling.exists() { Some(sibling) } else { None }
}

// ─── Shell ───────────────────────────────────────────────────────────────────

const LOGO: &str = r#"
    ██████╗ ██╗██╗  ██╗██╗   ██╗
    ██╔══██╗██║██║ ██╔╝██║   ██║
    ██████╔╝██║█████╔╝ ██║   ██║
    ██╔══██╗██║██╔═██╗ ██║   ██║
    ██║  ██║██║██║  ██╗╚██████╔╝
    ╚═╝  ╚═╝╚═╝╚═╝  ╚═╝ ╚═════╝
"#;

struct ShellContext {
    cwd: PathBuf,
    repo: Option<git2::Repository>,
}

impl ShellContext {
    fn new(start: PathBuf) -> Self {
        let repo = git2::Repository::discover(&start).ok();
        Self { cwd: start, repo }
    }

    fn cd(&mut self, target: &str) {
        let next = if std::path::Path::new(target).is_absolute() {
            PathBuf::from(target)
        } else {
            self.cwd.join(target)
        };
        match next.canonicalize() {
            Ok(p) if p.is_dir() => {
                self.cwd = p.clone();
                self.repo = git2::Repository::discover(&p).ok();
                println!("  → {}", self.cwd.display());
            }
            _ => println!("  [!] No existe o no es una carpeta: {}", next.display()),
        }
    }

    fn ls(&self, target: Option<&str>) {
        let dir = match target {
            Some(t) => {
                let p = if std::path::Path::new(t).is_absolute() {
                    PathBuf::from(t)
                } else {
                    self.cwd.join(t)
                };
                match p.canonicalize() {
                    Ok(p) => p,
                    Err(_) => { println!("  [!] No existe: {t}"); return; }
                }
            }
            None => self.cwd.clone(),
        };

        let mut entries: Vec<_> = match std::fs::read_dir(&dir) {
            Ok(e) => e.filter_map(|e| e.ok()).collect(),
            Err(_) => { println!("  [!] No se puede leer: {}", dir.display()); return; }
        };
        entries.sort_by_key(|e| e.file_name());

        println!();
        let mut found = false;
        for entry in &entries {
            if entry.path().is_dir() {
                println!("  {:>2}  {}/", "", entry.file_name().to_string_lossy());
                found = true;
            }
        }
        for entry in &entries {
            let path = entry.path();
            if path.extension().map(|e| e == "sch").unwrap_or(false) {
                let in_git = self.repo.as_ref().map(|r| {
                    r.workdir()
                        .and_then(|wd| path.strip_prefix(wd).ok())
                        .is_some()
                }).unwrap_or(false);
                let tag = if in_git { "[git]" } else { "     " };
                println!("  {tag}  {}", entry.file_name().to_string_lossy());
                found = true;
            }
        }
        if !found {
            println!("  (sin archivos .sch ni subdirectorios)");
        }
        println!();
    }

    fn prompt(&self) -> String {
        let dir = self.cwd.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.cwd.display().to_string());
        let repo_mark = if self.repo.is_some() { " (git)" } else { "" };
        format!("riku {dir}{repo_mark}> ")
    }

    fn repo_path(&self) -> PathBuf {
        self.repo.as_ref()
            .and_then(|r| r.workdir())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.cwd.clone())
    }

    fn resolve_file(&self, f: &str) -> String {
        let abs = if std::path::Path::new(f).is_absolute() {
            PathBuf::from(f)
        } else {
            self.cwd.join(f)
        };
        let repo = self.repo_path();
        abs.strip_prefix(&repo)
            .map(|rel| rel.to_string_lossy().to_string())
            .unwrap_or_else(|_| abs.to_string_lossy().to_string())
    }
}

fn shell_status_line(ctx: &ShellContext) -> String {
    let version = env!("CARGO_PKG_VERSION");
    let pdk = match (std::env::var("PDK_ROOT").ok(), std::env::var("PDK").ok()) {
        (Some(root), Some(pdk)) => {
            let sym = std::path::Path::new(&root).join(&pdk).join("libs.tech/xschem");
            if sym.exists() { format!("PDK: {pdk} [ok]") } else { "PDK: no detectado".to_string() }
        }
        _ => "PDK: no detectado".to_string(),
    };
    let repo_str = git2::Repository::discover(&ctx.cwd)
        .ok()
        .and_then(|r| r.workdir().map(|p| p.display().to_string()))
        .unwrap_or_else(|| "repo: no encontrado".to_string());
    format!("  v{version}  ·  {pdk}  ·  {repo_str}")
}

fn run_shell(_repo: PathBuf) -> Result<(), String> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let ctx = ShellContext::new(cwd);

    print!("{LOGO}");
    println!("{}", shell_status_line(&ctx));
    if ctx.repo.is_none() {
        println!("  [!] No se detectó repositorio Git. Usa 'cd <ruta>' para navegar a uno.");
    }
    println!("  'help' para ver los comandos. 'exit' para salir.\n");

    // ctx se mueve al loop; necesitamos mutabilidad
    let mut ctx = ctx;
    let mut rl = rustyline::DefaultEditor::new().map_err(|e| e.to_string())?;

    loop {
        let prompt = ctx.prompt();
        let line = match rl.readline(&prompt) {
            Ok(l) => l,
            Err(rustyline::error::ReadlineError::Interrupted | rustyline::error::ReadlineError::Eof) => break,
            Err(e) => return Err(e.to_string()),
        };

        let line = line.trim().to_string();
        if line.is_empty() { continue; }
        let _ = rl.add_history_entry(&line);

        let mut parts = line.splitn(2, ' ');
        let cmd = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("").trim();

        match cmd {
            "exit" | "quit" | "q" => break,
            "help" => print_shell_help(),
            "cd" => ctx.cd(if rest.is_empty() { "." } else { rest }),
            "ls" => ctx.ls(if rest.is_empty() { None } else { Some(rest) }),
            _ => dispatch_shell_command(&mut ctx, &line),
        }
    }

    println!("\n  Hasta luego.\n");
    Ok(())
}

fn print_shell_help() {
    println!();
    println!("  Navegación:");
    println!("    ls [ruta]                                     listar archivos .sch");
    println!("    cd <ruta>                                     cambiar directorio");
    println!();
    println!("  Git:");
    println!("    log [archivo.sch] [--semantic] [--limit <n>]  historial de commits");
    println!("    diff <commit_a> <commit_b> <archivo.sch>      diff semántico");
    println!("    diff ... --format visual                      diff visual en HTML");
    println!();
    println!("  Render:");
    println!("    render <archivo.sch>                          renderizar a SVG");
    println!("    gui [archivo.gds]                             abrir visor de escritorio");
    println!();
    println!("  Entorno:");
    println!("    doctor                                        verificar PDK y repo");
    println!("    exit                                          salir");
    println!();
}

fn dispatch_shell_command(ctx: &mut ShellContext, line: &str) {
    let mut args = vec!["riku"];
    args.extend(line.split_whitespace());

    match Cli::try_parse_from(&args) {
        Ok(parsed) => {
            let repo_path = ctx.repo_path();
            let result = match parsed.command {
                None | Some(Commands::Shell { .. }) => {
                    println!("  Ya estás en el shell.");
                    Ok(())
                }
                Some(Commands::Diff { commit_a, commit_b, file_path, repo: r, format }) => {
                    let effective_repo = if r == PathBuf::from(".") { repo_path } else { r };
                    let effective_file = ctx.resolve_file(&file_path);
                    run_diff(effective_repo, &commit_a, &commit_b, &effective_file, format)
                }
                Some(Commands::Log { file_path, repo: r, limit, semantic }) => {
                    let effective_repo = if r == PathBuf::from(".") { repo_path } else { r };
                    let effective_file = file_path.as_deref().map(|f| ctx.resolve_file(f));
                    run_log(effective_repo, effective_file.as_deref(), limit, semantic)
                }
                Some(Commands::Doctor { repo: r }) => {
                    run_doctor(if r == PathBuf::from(".") { repo_path } else { r })
                }
                Some(Commands::Render { file }) => {
                    let effective = if file.components().count() == 1 {
                        ctx.cwd.join(&file)
                    } else {
                        file
                    };
                    run_render(effective)
                }
                Some(Commands::Gui { file }) => {
                    let effective = file.map(|f| {
                        if f.components().count() == 1 { ctx.cwd.join(&f) } else { f }
                    });
                    run_gui(effective)
                }
            };
            if let Err(e) = result {
                eprintln!("  Error: {e}");
            }
        }
        Err(e) => {
            println!("  {}", e.to_string().lines().next().unwrap_or("comando no reconocido"));
        }
    }
}

// ─── System open ─────────────────────────────────────────────────────────────

fn open_file(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", &path.to_string_lossy()])
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(path).spawn().map_err(|e| e.to_string())?;
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string());
        if Command::new("xdg-open").env("DISPLAY", &display).arg(path).spawn().is_err() {
            eprintln!("[!] No se pudo abrir automáticamente. Abrelo manualmente: {}", path.display());
        }
        Ok(())
    }
}
