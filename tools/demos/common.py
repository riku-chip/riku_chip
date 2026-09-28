"""Base de los demos: un repo Git con historia reproducible (autor y fechas
fijos) que se empaqueta en un bundle (`git bundle`), y ediciones de
esquemáticos de Xschem y de simulaciones que usan varios demos.

Corre en el contenedor iic-osic-tools (xschem, ngspice, KLayout y los PDK).
"""
import os
import re
import shutil
import subprocess
from datetime import datetime, timedelta, timezone

AUTHOR = ("Riku Demo", "demo@riku.invalid")
START = datetime(2026, 3, 2, 9, 30, tzinfo=timezone.utc)
TOOLS = "/foss/tools/bin"


def sh(cmd, cwd=None, env=None, check=True):
    e = dict(os.environ)
    e["PATH"] = TOOLS + ":" + e.get("PATH", "")
    e.update(env or {})
    r = subprocess.run(cmd, cwd=cwd, env=e, shell=isinstance(cmd, str), capture_output=True, text=True)
    if check and r.returncode != 0:
        raise SystemExit(f"falló {cmd}:\n{r.stdout}\n{r.stderr}")
    return r.stdout


class Repo:
    """Un repo nuevo en `path`. Cada commit es medio día después del anterior."""

    def __init__(self, path):
        if os.path.exists(path):
            shutil.rmtree(path)
        os.makedirs(path)
        self.path = path
        self.step = 0
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", AUTHOR[0])
        self.git("config", "user.email", AUTHOR[1])

    def git(self, *args):
        when = (START + timedelta(hours=13 * self.step)).isoformat()
        env = {
            "GIT_AUTHOR_NAME": AUTHOR[0], "GIT_AUTHOR_EMAIL": AUTHOR[1], "GIT_AUTHOR_DATE": when,
            "GIT_COMMITTER_NAME": AUTHOR[0], "GIT_COMMITTER_EMAIL": AUTHOR[1], "GIT_COMMITTER_DATE": when,
        }
        return sh(["git", *args], cwd=self.path, env=env)

    def file(self, rel):
        p = os.path.join(self.path, rel)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        return p

    def write(self, rel, text):
        with open(self.file(rel), "w", encoding="utf-8", newline="\n") as f:
            f.write(text)

    def read(self, rel):
        with open(os.path.join(self.path, rel), encoding="utf-8") as f:
            return f.read()

    def copy(self, src, rel):
        shutil.copyfile(src, self.file(rel))

    def commit(self, message, tag=None):
        self.step += 1
        self.git("add", "-A")
        self.git("commit", "-q", "-m", message)
        if tag:
            self.git("tag", tag)
        print(f"  {self.git('log', '-1', '--format=%h %s').strip()}{'  [' + tag + ']' if tag else ''}")

    def branch(self, name):
        self.git("checkout", "-q", "-b", name)

    def checkout(self, name):
        self.git("checkout", "-q", name)

    def merge(self, name, message, resolve=None):
        """Merge sin fast-forward. Un conflicto (un GDS binario: Git no los
        fusiona) se resuelve con `resolve()`, que deja los archivos como deben
        quedar, como lo haría el diseñador a mano."""
        self.step += 1
        when = (START + timedelta(hours=13 * self.step)).isoformat()
        env = {"GIT_AUTHOR_DATE": when, "GIT_COMMITTER_DATE": when}
        r = sh(["git", "merge", "-q", "--no-ff", name, "-m", message], cwd=self.path, env=env, check=False)
        if "CONFLICT" in r or self.git("status", "--porcelain").strip():
            assert resolve, f"conflicto en el merge de {name}"
            resolve()
            self.git("add", "-A")
            sh(["git", "commit", "-q", "--no-edit", "-m", message], cwd=self.path, env=env)
        print(f"  {self.git('log', '-1', '--format=%h %s').strip()}")

    def bundle(self, out):
        os.makedirs(os.path.dirname(out), exist_ok=True)
        self.git("bundle", "create", "-q", out, "--all")
        print(f"  → {out} ({os.path.getsize(out) // 1024} KiB)")


# ---- Xschem ----

def set_param(sch, inst, key, value):
    """`key=value` en las propiedades de la instancia `inst` (`name=M1`)."""
    m = re.search(r"\{name=" + re.escape(inst) + r"\b[^}]*\}", sch)
    assert m, inst
    block = m.group(0)
    new, n = re.subn(r"(^|\n|\{| )" + re.escape(key) + r"=[^\n }]*", lambda g: g.group(1) + f"{key}={value}", block, count=1)
    assert n == 1, (inst, key)
    return sch[: m.start()] + new + sch[m.end():]


def move_all(sch, dx, dy):
    """Todo el esquemático desplazado (componentes `C` y cables `N`): un
    reordenamiento visual, sin cambio de circuito."""
    def c_line(m):
        return f"{m.group(1)}{float(m.group(2)) + dx:g} {float(m.group(3)) + dy:g}"

    def n_line(m):
        x1, y1, x2, y2 = (float(v) for v in m.groups())
        return f"N {x1 + dx:g} {y1 + dy:g} {x2 + dx:g} {y2 + dy:g}"

    sch = re.sub(r"^(C \{[^}]*\} )(-?[\d.]+) (-?[\d.]+)", c_line, sch, flags=re.M)
    return re.sub(r"^N (-?[\d.]+) (-?[\d.]+) (-?[\d.]+) (-?[\d.]+)", n_line, sch, flags=re.M)


def simulate(repo, xschem_dir, tb, sim_dir):
    """Netlist del testbench con xschem y corrida con ngspice; el `.raw`
    queda en `sim_dir` con la fecha fija (así el bundle es reproducible)."""
    env = {"PDK_ROOT": os.environ.get("PDK_ROOT", "/foss/pdks"), "PDK": "sky130A"}
    xdir, sdir = os.path.join(repo.path, xschem_dir), repo.file(os.path.join(sim_dir, "x"))
    sdir = os.path.dirname(sdir)
    sh(["xschem", "-q", "-x", "-b", "-s", "-n", "--netlist_path", sdir, tb], cwd=xdir, env=env)
    spice = os.path.join(sdir, tb.replace(".sch", ".spice"))
    log = sh(["ngspice", "-b", os.path.basename(spice)], cwd=sdir, env=env)
    os.remove(spice)
    raw = os.path.join(sdir, tb.replace(".sch", ".raw"))
    data = open(raw, "rb").read()
    data = re.sub(rb"Date: [^\n]*\n", b"Date: Mon Mar  2 09:30:00  2026\n", data, count=1)
    open(raw, "wb").write(data)
    return {k: float(v) for k, v in re.findall(r"^(\w+)\s+=\s+([-+\d.eE]+)", log, re.M)}
