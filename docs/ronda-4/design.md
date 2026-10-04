# Ronda 4: diseño

Cómo se cumple [`requirements.md`](requirements.md). Código revisado el 2026-10-04 sobre `main` (`b137928`).

## D16. Reglas `device … +tipos`

**Causa.** `devices/rules.rs::device_models` arma, por tipo de Magic, `models: Vec<(modelo, Vec<Cond>)>` en el orden del `.tech`. `Cond::parse` solo entiende `w`/`l` con `<`, `>`, `<=`, `>=`; el token `+npn,pnp` se descarta. `DeviceType::model(w, l)` toma el primero cuyas condiciones se cumplen: para `mvnfet`/`mvpfet` es `Ignore` (la primera línea, sin condiciones de W/L).

**Cambio.**

```rust
pub struct ModelRule { pub name: String, pub conds: Vec<Cond>, pub near: Vec<String> }  // reemplaza (String, Vec<Cond>)
pub struct DeviceType { …, pub models: Vec<ModelRule>, … }

impl DeviceType {
    /// El primero cuyas condiciones de W y L se cumplen y, si pide `+tipos`,
    /// que tenga alguno de esos tipos junto a la compuerta (`near_ok`).
    pub fn model_for(&self, w_um: f64, l_um: f64, near_ok: &dyn Fn(&[String]) -> bool) -> Option<&str>
    pub fn model(&self, w_um: f64, l_um: f64) -> &str   // igual que hoy pero salteando las que piden `+tipos`
}
```

- **Lectura:** en `device_models`, un token que empieza con `+` es `near` (`+npn,pnp` → `["npn", "pnp"]`, canónicos). Nada más cambia en la lectura de terminales y sustrato (los `+…` van al final de la línea).
- **Evaluación** (en `extract.rs`, donde se elige el modelo de cada compuerta): `near_ok(tipos)` es verdadero si algún polígono de esos tipos toca la caja de la compuerta agrandada en un paso de grilla. Se usa el mismo `LayerPolys` que ya resuelve qué tipo hay en un punto.
- **`Ignore`:** si el modelo elegido es `Ignore`, el transistor no se lista (es lo que hace Magic: geometría que forma parte de otro dispositivo).
- **`model(w, l)`** (lo usan otros lugares que no tienen la geometría a mano) saltea las reglas con `near`: para los transistores comunes no cambia nada.
- **`devices_generated.rs`** es la copia compilada del `.tech`; no cambia (se lee igual).

**Verificación (R16.3):** `tools/verify/devices/` y `tools/verify/nets/` (contra Magic) y las pruebas de `riku-mod-layout`; las 437 celdas `sky130_fd_sc_hd` con `lvs_probe` antes y después (mismas netlists); el demo `ota` con el mismo `riku lvs`.

**Ejemplos (R16.4):** `examples/nets.rs` y `examples/lvs_probe.rs` leen un `.mag` con `mag::collect(&bytes, &path, Some(&DiskFiles::new(carpeta)))`.

## D17. Demo `inversor`

**Script:** `tools/demos/inversor.py` (como `ota.py`, con `common.Repo`), salida `examples/demos/inversor.bundle`, embebido en `riku/src/cli/demo.rs` como `Demo { name: "inversor", about: "demo.about.inversor", … }`.

**Archivos del repo:**

```text
LICENSE / NOTICE          Apache-2.0 de iic-osic-tools, con la atribución
README.md                 qué probar
xschemrc                  como en ota (símbolos del PDK)
xschem/inv.sch inv.sym tb_inv.sch
layout/inv.mag            la celda de arriba
layout/sky130_fd_pr__nfet_g5v0d10v5_H9JWFY.mag
layout/sky130_fd_pr__pfet_g5v0d10v5_5AEDG4.mag
```

Los `.mag` en `layout/` y los `.sch` en `xschem/`: el par se empareja por nombre (`inv.sch` ↔ `inv.mag`), igual que en `ota`.

**Ediciones de `.mag`, como texto** (son pocas líneas y así quedan deterministas). Una función por caso, cada una comprobada por `riku` y por Magic (R17.6):

| # | Commit | Edición | Qué se ve en Riku |
|---|---|---|---|
| 1 | Inicial | — | LVS coincide (con D16) |
| 2 | `metal2` más ancho en `out` | `<< metal2 >>`: un `rect` más ancho | la capa `metal2` por nombre en `diff` y en el visor |
| 3 | nfet más ancho | en `…nfet…H9JWFY.mag`: `mvnmos`, `mvndiff` y sus contactos 1 µm más altos (W 2 → 3) | `riku diff layout/inv.mag`: la sub-celda cambió, vista desde `inv`; el transistor con su W nuevo; LVS deja de coincidir (`M9 w 2 ≠ 3`) |
| 4 | esquemático: `M9 W=3` | `inv.sch` | LVS vuelve a coincidir |
| 5 | puertos con clase | `port 1 n` → `port 1 n default input` (`in`), `port 2 n` → `… output` (`out`) | cambio de puerto, sin geometría |
| 6 | un abierto en `out` | quitar el `rect` de `metal2` que une la vía de arriba con la de abajo | "1 abierto"; LVS empeora (redes sin pareja) |
| 7 | arreglo | volver a poner el `rect` | LVS mejora |
| 8 | re-grabación | solo los `timestamp` (celda y padres) | sin cambios |
| 9 | rama `wider-pmos`: pmos más ancho | `…pfet…5AEDG4.mag` y `inv.sch` (`W=8 → 10`) | dos archivos, LVS coincide en la rama |
| 10 | merge | `git merge --no-ff wider-pmos` | |

Tag `v1.0` en el commit 7 (diseño arreglado). Las medidas exactas (cuánto se agranda cada rect para dar el W pedido) se calculan en el script a partir de la grilla del `.mag` (`magscale 1 2`: 1 unidad = 0,005 µm) y se confirman con `lvs_probe` (W de cada transistor).

**Comprobación de cada commit (R17.5, R17.6):** el script, al final, corre en el repo generado `riku log --lvs -n 20 -f json` y compara el veredicto y la transición de cada commit con una tabla esperada; y `magic -dnull -noconsole` con `extract all` + `ext2spice lvs` + Netgen (el flujo de `tools/verify/nets/`) en cada commit, que debe dar el mismo veredicto. Si algo no coincide, el script falla y no escribe el bundle.

**README del repo:** como el de `ota`: una lista de comandos con una línea de qué se ve (`riku log --graph --lvs`, `riku diff HEAD~3 HEAD layout/inv.mag`, `riku show <commit del abierto>`, `riku gui .` → Historial → LVS).

## D18. Demo `chip`

### Script y contenido

`tools/demos/chip.py <carpeta>` arma el repo (no un bundle) con KLayout (`klayout -b -r` con un script Python de `klayout.db`, como `sram.py`):

| # | Commit | Edición con `klayout.db` | Para qué |
|---|---|---|---|
| 1 | Inicial `v1.0` | el GDS de `sky130_sram_macros` tal cual | base |
| 2 | Bitcell | en la celda hoja de la matriz, un rect de `met1` 10 nm más ancho | un cambio en una celda instanciada miles de veces; la huella jerárquica lo encuentra sin aplanar todo |
| 3 | *Straps* en la celda de arriba | rects de `met4` sobre el macro | el caso de la celda de arriba (~1,7 s medido en la `sram` chica) |
| 4 | Renombre | una celda de control con otro nombre, sin cambiar su contenido | renombre puro |
| 5 | Rama `pin-fix`: un pin movido | la etiqueta y el rect de un pin de datos, 2 µm | puerto que cambia de lugar |
| 6 | Merge | | |

El GDS se reescribe con KLayout en cada paso (mismo orden y opciones), para que Git comprima bien los cambios. **Tamaño:** se mide el repo con `git count-objects -vH` después de `git gc --aggressive`; si pasa de 40 MB (R18.5), se sacan commits. Sin LFS.

**Licencia (R18.7):** antes de publicar, confirmar la licencia de `sky130_sram_macros` en su repositorio de origen y copiarla al repo con la atribución. Si no fuera compatible, el demo usa el macro más chico que sí lo sea.

### `riku demo chip`

```rust
struct Demo { name, about, source: Source }
enum Source { Bundle(&'static [u8]), Remote { url: &'static str, size: &'static str } }
const CHIP_URL: &str = "https://github.com/riku-chip/riku-demo-chip.git";
```

- **Clonar:** `git clone --quiet <url> <dir>` y, como con los bundles, las ramas locales y sin el remoto (`create` se reusa, con la URL en lugar del bundle temporal). `RIKU_DEMO_CHIP_URL` reemplaza la URL (pruebas, espejos).
- **`riku demo` sin nombre:** solo los `Bundle` (R18.2); al final, una línea: "`riku demo chip` descarga un macro de 1 KB (~N MB)".
- **Errores (R18.3):** si `git clone` falla, se borra la carpeta y el mensaje dice la URL y el error de Git.
- **Prueba sin red (NF3):** la prueba arma un repo local con `git init`, lo apunta con `RIKU_DEMO_CHIP_URL=file://…` y comprueba el clonado, las ramas y que no queda el remoto.

### Publicación (R18.8)

El script deja el repo listo en una carpeta. Crear `riku-chip/riku-demo-chip` en GitHub y hacer el `push` lo hace el usuario (o se hace con su permiso, en ese momento). Hasta entonces, `riku demo chip` falla con el mensaje de R18.3.

### Mediciones (R18.6)

Con el binario de release, en el repo clonado y en `/tmp` (no en un montaje lento, ver `desarrollo.md`): `riku log -n 10`, `riku show` de cada commit, `riku diff v1.0 HEAD`, `riku status`, y el visor (abrir el GDS y el Historial). Cada uno con `RIKU_PROFILE=1` para la etapa más cara. Se anotan en el README del repo y en `docs/desarrollo.md`.

## D19. Documentación

- `README.md` (raíz) y `docs/cli.md`: `riku demo` con `ota`, `sram`, `inversor` y `chip` (este último se descarga).
- `docs/desarrollo.md`: regenerar los bundles (`python3 tools/demos/inversor.py`) y el repo del chip (`python3 tools/demos/chip.py /tmp/riku-demo-chip`), y los tiempos.
- `docs/pendientes.md`: quitar las dos filas de demos.

## Riesgos

| Riesgo | Qué se hace |
|---|---|
| El cambio de `+tipos` mueve algún otro transistor de SKY130, GF180 o IHP | las verificaciones de `tools/verify/` y las 437 celdas, antes y después |
| Un `.mag` editado a mano que Magic no acepta | cada commit pasa por Magic (D17) |
| El repo del chip pesa demasiado | medir; menos commits o cambios más chicos |
| La licencia del macro | se confirma antes de publicar; alternativa: otro macro |
| `git` sin red en la máquina del usuario | error claro con la URL (R18.3) |
