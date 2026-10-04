# Ronda 5: diseño

Cómo se cumple [`requirements.md`](requirements.md). Código revisado el 2026-10-04 sobre `main` (`4b6c5e9`).

## Idea

Hoy una celda se extrae entera: se aplana todo lo que tiene debajo y se arman sus redes de cero. La extracción jerárquica pone **un resumen por celda**, guardado por la huella de su contenido:

```text
  chip ──────────── CellNets(chip)   propio + uniones con sus hijas
   ├─ bank ──────── CellNets(bank)   ← reusado si su huella no cambió
   │   └─ array ─── CellNets(array)  8 192 instancias: vecindades memorizadas
   │       └─ bitcell  CellNets(bitcell)   ← lo único que se extrae otra vez
   └─ control ───── CellNets(control) ← reusado
```

El resumen de una celda guarda:

- sus **pedazos propios**, con su red;
- sus **transistores propios**;
- sus **instancias**, con la huella de la hija y su transformación;
- sus **pines**: qué redes de las hijas usa, y con qué red propia se unen.

El de la hija no se copia: se busca por su huella.

**Para extraer una celda** solo se mira:

- su geometría propia;
- los pedazos de las hijas que tocan algo del padre, o que tocan a una hermana;
- las etiquetas del padre.

**Todo lo que ya existe se reusa:**
- La unión de pedazos y el *union-find* (`extract::build_with`), con los pedazos de las hijas como "pedazos ajenos" con una red ya fijada.
- La huella jerárquica (`prints::tree_prints`).
- El directorio y las reglas de la caché de diffs (`DiffCache`).

Todo va en un módulo nuevo, `riku-mod-layout/src/nets/hier/`:

| Archivo | Qué hace |
|---|---|
| `mod.rs` | La API: `HierNets`, `extract`, `flatten`, `spice` |
| `key.rs` | La huella de la netlist de cada celda (`NetKey`) |
| `cell.rs` | El resumen de una celda (`CellNets`) a partir de su geometría y de los resúmenes de sus hijas |
| `query.rs` | Los pedazos de una hija dentro de una caja, bajando por la jerarquía |
| `raw.rs` | La geometría cruda de una celda por capa, en una grilla: los pines y el contexto |
| `conflict.rs` | Cuándo una instancia depende del contexto y se aplana (R20.3) |
| `memo.rs` | La memoria y el disco (R21): celdas, vecindades y comparaciones |
| `deps.rs` | Consultas registradas, interfaz y corte temprano (D20.6) |
| `flatten.rs` | Pasar de jerárquica a `Netlist` plana (oráculo, visor, LVS plano) |
| `spice.rs` | La SPICE jerárquica (R23.3) |
| `compare.rs` | Abiertos, cortos y renombres por celda (R22) |

## El modelo: grafo, árbol, caché y DOM virtual

Cuatro maneras de mirar lo mismo. Cada una aporta una pieza concreta del diseño.

### Grafo: la jerarquía es un DAG con aristas transformadas

```text
            chip
          ╱      ╲
       bank      control           nodos  = celdas (contenido)
         │  ╲       │              aristas = instancias (transformación)
       array  ╲     │
         │ ×8192 ╲  │
       bitcell ── nand2            nand2: un nodo, muchos padres
```

- **Es un DAG, no un árbol.** Una celda usada 8 192 veces es **un** nodo con 8 192 aristas entrantes. La extracción es por nodo, no por instancia: el costo va con las celdas distintas, no con las instancias.
- **El orden es topológico,** de las hojas a la raíz. Las celdas sin dependencias entre sí (las de un mismo nivel) van en paralelo.
- **Las redes son un grafo dentro de cada nodo.** Cada nodo tiene un grafo de redes: los pedazos que se tocan, unidos con *union-find*. El padre agrega aristas que cruzan las fronteras:
  - **pin–pin**, entre hermanas: las vecindades;
  - **propio–pin**, entre el padre y una hija.

  Una netlist jerárquica es un **grafo de grafos**: cada nodo expone sus **puertos** (las redes que otro puede tocar) y esconde lo demás.
- **Índice inverso (padres de cada celda).** Cuando una celda cambia, se marcan **sucias** ella y las que la alcanzan por aristas inversas (su "espina" hasta la raíz). Solo esas se recalculan. Es la propagación de un sistema de *build*.

### Árbol de Merkle: identidad por contenido

- **`NetKey` es un árbol de Merkle** sobre el DAG, como los objetos de Git: el mismo contenido da la misma clave, en cualquier commit, archivo o lado de un diff.
- **La memoria es un almacén direccionado por contenido** (*hash-consing*). Dos subárboles iguales son el mismo objeto (`Arc<CellNets>`), y comparar dos subárboles es comparar dos `u128`.
- **Lo que no tiene clave no se guarda** (un ciclo, una repetición enorme): se calcula y se tira.

### Caché: memoización con corte temprano

**Tres niveles, cada uno direccionado por contenido:**

| Nivel | Clave | Valor | Dónde |
|---|---|---|---|
| Celda | `NetKey` | `CellNets` | memoria → disco |
| Arista (vecindad) | `(NetKey, NetKey, Transform)` | pares de redes que se tocan | memoria |
| Comparación | `(NetKey A, NetKey B)` | los cambios de esa celda (D22) | memoria → disco |

**Corte temprano.** Es lo que no da un Merkle solo. Con `NetKey` sola, un cambio en el **interior** de un bitcell (un contacto movido que no toca el borde) cambia la clave de `array`, `bank` y `chip`, y obliga a rearmar toda la espina. Pero el padre no depende de **todo** el hijo: depende solo de **lo que le preguntó**.

- **Se registra lo que el padre consulta.** Al armar un padre, cada consulta a una hija (`pieces_in` con su caja y sus tipos, las candidatas al sustrato, la huella de contexto) se anota con el **hash de la respuesta**. Es lo mismo que hace `RecordingFiles` en la caché del LVS de la ronda 3.
- **La interfaz de la hija vista por ese padre** (`Iface`) es el hash de esas respuestas, en orden.
- **Cuando la hija cambia,** antes de rearmar el padre se **repiten sus consultas** contra la hija nueva:
  - **si todas dan lo mismo,** el resultado del padre sigue valiendo. Se reusa cambiando solo la `NetKey` de esa hija en `instances`. La espina **se corta ahí**, y el abuelo ve un padre con la misma interfaz;
  - **si alguna difiere,** el padre se rearma, y el corte se prueba de nuevo un nivel más arriba.
- **Por qué es seguro:** las respuestas incluyen el número de red de cada pedazo. Un corto **dentro** de la hija entre dos redes expuestas hace que dos pedazos respondan con la misma red: la respuesta cambia y no hay corte.

  Para que un cambio interior no renumere las redes expuestas, cada red se numera de forma estable: por su pedazo de menor posición, no por el orden de creación.
- **Dónde se guarda:** `CellNets` del padre guarda, por instancia distinta (hija y transformación), sus consultas y el hash de cada respuesta (`deps`). Para una matriz son las de unas pocas vecindades.

Es la estrategia *red-green* de Salsa (rust-analyzer), o la de Bazel y Adapton.

### DOM virtual: reconciliar dos árboles con claves

La comparación de dos versiones (D22) es la **reconciliación** de React:

| React | Riku |
|---|---|
| Componente | Celda |
| `props` | Transformación de la instancia |
| `key` | `NetKey` |
| `React.memo`: mismas props, no se re-renderiza | La misma `NetKey`, no se compara el subárbol |
| Hijos con `key`: se emparejan por clave, aunque se muevan | Instancias gemelas: la misma `NetKey` y la misma transformación, como multiconjunto |
| Hijos sin `key`: se emparejan por posición | Instancias de una hija que cambió: el mismo nombre y la misma transformación |
| *Patch*: la lista mínima de cambios | `NetChange` y `DeviceChange` por celda, solo en la espina |
| Render perezoso, virtualización de listas | Extraer recién cuando alguien pregunta: el visor pide la celda abierta; una consulta baja solo por las ramas que tocan su caja |

**Reconciliación recursiva:**

```rust
fn reconcile(a: NetKey, b: NetKey, memo: &Memo) -> Arc<Vec<Change>> {
    if a == b { return EMPTY }                       // React.memo
    if let Some(p) = memo.compare.get(&(a, b)) { return p }   // misma comparación en otro commit del log
    // los hijos: gemelos por clave, el resto por nombre y transformación
    // en los que difieren, reconcile(...) recursivo; en este nivel, classify (D22)
}
```

- **Lo que nunca se recorre:** la recursión solo baja por pares distintos. Un `log` de 1 000 commits que tocan siempre el mismo bitcell compara `bitcell` y su espina; nunca `control`.
- **El *patch* se memoriza por `(a, b)`:** `log`, `show` y `diff` de rangos que se superponen comparten resultados. `riku diff v1.0 HEAD` reusa los pares que ya calculó `log`.
- **Perezoso y cancelable, como Fiber.** El trabajo es por nodo, en orden topológico, con un `CancelToken` entre nodos: el visor que cambia de celda deja de extraer la anterior. Y es perezoso: solo se arma lo que la pregunta necesita (`reconcile` no arma la parte de la espina que el corte temprano ya resolvió).

### Qué cambia en el diseño por esto

| Idea | Dónde |
|---|---|
| Índice inverso y espina sucia | D22.2 |
| Numeración estable de las redes expuestas | D20.1 paso 5, D20.5 |
| Consultas registradas y corte temprano (`deps`, `Iface`) | D20.6 (nuevo), D21 |
| Memoria de comparaciones `(NetKey, NetKey)` | D21, D22 |
| `reconcile` recursivo con gemelos por clave | D22 |
| Cancelación entre nodos | D20.1, D23 (visor) |

## D20. El resumen de una celda

```rust
/// Una red vista desde una celda: una propia (`path` vacío) o la de una
/// hija, más abajo, que nadie expuso todavía. `path`: índices de instancia
/// desde la celda.
#[derive(Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct NetRef { pub path: SmallPath, pub net: u32 }   // SmallPath = Vec<u32>

#[derive(Clone, Serialize, Deserialize)]
pub struct Inst {
    pub cell: String,          // nombre en esta librería (para mostrar y para la SPICE)
    pub key: NetKey,           // el resumen de la hija
    pub at: Transform,         // origen, rotación (múltiplo de 90°), reflejo; una por repetición de un AREF
    pub bbox: [f64; 4],        // la de la hija transformada (unidades de la librería)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CellNets {
    pub key: NetKey,
    pub nets: Vec<Net>,                       // las propias: `Net` de hoy (nombre, etiquetas, port, substrate, bbox)
    pub pieces: Vec<NetPiece>,                // los propios (y los de instancias aplanadas por contexto)
    pub devices: Vec<(Device, Terminals)>,    // los propios (ídem)
    pub resistors: Vec<(Resistor, [usize; 2])>,
    pub labels: Vec<NetLabel>, pub label_nets: Vec<Option<usize>>,
    pub instances: Vec<Inst>,                 // las no aplanadas
    pub pins: Vec<(NetRef, u32)>,             // red de una hija (o más abajo) → red propia; ordenado
    pub sub_open: Vec<u32>,                   // redes candidatas al sustrato, sin decidir (D20.4)
    pub sub_pin: Option<u32>,                 // la red "sustrato global" de esta celda (cuerpos sin pozo)
    pub type_bbox: Vec<(String, [f64; 4])>,   // por tipo, la caja de los pedazos de toda la sub-jerarquía (para podar)
    pub stats: Stats,                         // transistores y redes de toda la sub-jerarquía, polígonos, aplanadas por contexto
    pub deps: Vec<Deps>,                      // por instancia distinta (hija, transformación): lo consultado (D20.6)
    pub iface: u64,                           // hash de lo que esta celda responde a sus padres (D20.6)
    pub warnings: Vec<String>,
}

/// Lo que un padre le preguntó a una hija y el hash de cada respuesta.
#[derive(Clone, Serialize, Deserialize)]
pub struct Deps { pub child: NetKey, pub at: Transform, pub asked: Vec<(Query, u64)> }
```

### D20.1. Cómo se arma (`cell.rs`)

Se arma de abajo hacia arriba, por niveles: las hojas primero, en paralelo con `rayon`. Para cada celda `C`:

1. **Geometría propia.** Los polígonos de profundidad 0 (`get_polygons().depth(0)`), más los de las instancias que [`conflict`](#d204-contexto-conflictrs) manda aplanar. Con eso, la región de cada tipo se arma como hoy en `cell_nets_in`: en Magic, por el nombre de la capa; en un GDS, con `RegionEval` y `paint_order`. Los transistores propios, con `devices::extract` / `extract_magic`.

2. **Pedazos ajenos.** Son los pedazos de las hijas que pueden unirse a algo:
   - **Con el padre:** para cada pedazo propio, `query` busca en las instancias cuya caja del tipo (`type_bbox`) toca su caja agrandada en `touch`. Trae los pedazos de los tipos que `connect` une con él, con su `NetRef` y transformados a `C`.
   - **Con las etiquetas propias:** los pedazos bajo cada etiqueta (su punto y, si es un puerto de Magic, su rectángulo).

3. **Unión.** `build_with` recibe los pedazos ajenos aparte: entran en la unión de cada grupo de `connect`, como los propios, pero cada uno empieza unido al nodo de su `NetRef`, y no se listan como pedazos de `C`. Así, la regla de "se tocan" es la misma que en la extracción plana, con el mismo medio nanómetro.

4. **Hermanas.** Para cada par de instancias cuyas cajas se tocan (una grilla de cajas) se piden los pares `(NetRef de una, NetRef de la otra)` que se tocan ([D20.3](#d203-vecindades-memorizadas)), y se unen sus nodos.

5. **Pines.** Cada `NetRef` que apareció en 2, 3 o 4 se resuelve a una red de `C`. Quedan en `pins` y la red se crea si no existía. Las redes de las hijas que nada tocó **no se copian**: siguen siendo internas de la hija. De esto sale el ahorro en memoria y en tiempo.

   **Numeración estable:** las redes de `C` se numeran por su pedazo de menor posición (`y`, luego `x`, luego tipo), no por el orden de creación. Así un cambio interior no renumera las redes que otros ven, y el corte temprano (D20.6) funciona.

6. **Nombres y pines de la celda:** las etiquetas propias, como hoy. Un pin de un GDS se reconoce por la capa de pines bajo la etiqueta, que se busca en `raw` bajando a las hijas.

**Transformaciones.** Una instancia con magnificación ≠ 1 o con un ángulo que no es múltiplo de 90° **se aplana** dentro del padre (como un conflicto): W y L cambiarían y la grilla dejaría de ser exacta. No pasa en los PDK abiertos.

**Ciclos.** Una referencia circular, o una celda sin huella (`TreePrints::tree == None`), se extrae aplanada como hoy (`check_acyclic` ya avisa del ciclo).

### D20.2. Consultas a una hija (`query.rs`)

```rust
/// Los pedazos de `cell` (y de su sub-jerarquía) de esos tipos que tocan
/// `area` (coordenadas de `cell`), con la red de cada uno vista desde `cell`.
fn pieces_in(nets: &Memo, cell: &CellNets, area: [f64; 4], types: &[String]) -> Vec<(NetRef, OwnedPolygon)>
```

- **Pedazos propios:** se buscan en la grilla por tipo de `cell`. La grilla se arma la primera vez que se pide y se guarda en memoria, no en disco.
- **Instancias:** las que tocan `area` se recorren hacia abajo: `area` se lleva a las coordenadas de la hija con la transformación inversa, y lo que vuelve se transforma de regreso.
- **Redes de lo que vuelve:** si `cell.pins` tiene la `NetRef` de la hija, es esa red propia (`path` vacío). Si no, `NetRef { path: [i] ++ path_hija, net }`.
- **Poda:** `type_bbox` corta las ramas que no tienen ese tipo en esa zona.

### D20.3. Vecindades memorizadas

```rust
type Neighbourhood = (NetKey, NetKey, Transform);  // la segunda instancia vista desde la primera
fn touching(memo: &Memo, a: &CellNets, b: &CellNets, rel: Transform) -> Arc<Vec<(NetRef, NetRef)>>
```

**Cálculo:**
1. La caja común de las dos, agrandada en `touch`.
2. `pieces_in` de cada una en esa caja, la segunda llevada a las coordenadas de la primera.
3. La unión por grupo de `connect` (la misma de `build_with`) da los pares que se tocan.

**Memoria.** El resultado se guarda en `Memo` con la clave `Neighbourhood`, que no depende de dónde está el par. Una matriz de 8 192 bitcells espejados tiene unas pocas vecindades distintas (derecha, arriba, en diagonal, cada una con su reflejo): se calculan unas pocas y se aplican 8 192 veces.

**Caso malo:** una instancia enorme que cubre a todas (una grilla de alimentación como celda aparte). Da una vecindad distinta por instancia. Se mide (`RIKU_PROFILE`: "vecindades: N calculadas, M reusadas") y se anota en Riesgos.

### D20.4. Contexto (`conflict.rs`)

> **Lo que salió al medir (2026-10-04, T21–T22).** La idea de abajo, aplanar toda instancia que comparta un par de capas con su entorno, se descartó. En la SRAM marca 3 424 de 3 574 instancias: el pozo N entra en casi todas las reglas, y así no se ahorra nada. Lo que funcionó es lo que hace Magic al leer un GDS: cada celda se evalúa sola, con tres arreglos que cubren lo que en la práctica depende del entorno.
>
> 1. **Las celdas chicas se meten en su padre** (menos de 256 polígonos aplanados, como `gds flatglob`): contactos y transistores sueltos de OpenRAM no son nada solos (un `licon` sin la difusión de abajo).
>
>    | Umbral | Resultado |
>    |---|---|
>    | 0, 32, 64, 128 | No coincide con la plana |
>    | 192, 256, 512, 1024 | Coincide con la plana en la SRAM |
>
>    Por omisión, 256 (`RIKU_HIER_INLINE`).
> 2. **Cierres de las reglas** (`grow g` + `shrink g`, `DeviceRules::bridge`): el pozo P de SKY130 une pozos de celdas distintas a menos de 0,84 µm. Dos pedazos del mismo tipo a menos de esa distancia se unen.
> 3. **Sustrato:**
>    - los pozos P de una hija quedan como candidatos (`open`) hasta que un `dnwell` de arriba los excluye o la raíz los une al sustrato;
>    - los cuerpos que una hija sola cree "en el sustrato" (`sub_points`) van al pozo que el padre tenga en ese punto: el pozo P aislado de un `dnwell` dibujado arriba.
>
> Con eso, la plana y la jerárquica son la misma netlist en todo lo comparado:
> - la SRAM de `examples/`;
> - el inversor (Magic, también sin meter nada);
> - el OTA;
> - `port_data`, `port_address` y `control_logic_rw` de la macro.
>
> La macro de 1 KB entera, que la plana no extrae, sale en 6–7 s con 450 MB. Lo que sigue abajo queda como el diseño original.

Lo que se extrae dentro de una celda no puede depender de lo que la rodea (R20.3). Hay tres casos.

**1. En Magic**, la capa ya es el tipo y Magic tampoco mezcla tipos entre celdas: el único caso es el sustrato (punto 3).

**2. En un GDS**, de las reglas sale una tabla de **pares de capas que interactúan**:

```rust
impl DeviceRules {
    /// Pares de capas GDS (distintas) que intervienen juntas en la región de
    /// algún tipo (`and`, `and-not`, `or` con otra capa en `cifinput`) o en
    /// un transistor (compuerta sobre difusión, `sub`, `+tipos`).
    fn context_pairs(&self) -> Vec<((u32, u32), (u32, u32), f64 /* alcance, µm: el mayor grow */)>;
}
```

- **Prueba gruesa:** para cada instancia, si la geometría del padre o de una hermana en la capa `X` se acerca a menos del alcance a la de la instancia en la capa `Y`, siendo `(X, Y)` un par. Se usa la **huella de contexto** de la hija: por capa de algún par, hasta 64 cajas que cubren su geometría de toda la sub-jerarquía. Se calcula con la clave y viaja en `CellNets`.
- **Prueba fina:** solo si la gruesa da positivo, con la geometría cruda de `raw.rs` en esa zona.
- **Si interactúan:** la instancia se aplana dentro del padre (sus polígonos pasan a "propios") y se cuenta en `stats`.
- **Sin conflicto:** dos `nwell` que se superponen (la misma capa) no son un conflicto. Solo se unen, y eso ya lo hace la conectividad.

**3. Sustrato.**
- **En la hija:** los pedazos de tipos del sustrato (`pwell`, tomas P) que nada de la hija excluye **no se unen al sustrato**. Quedan como redes candidatas (`sub_open`). Los cuerpos sin pozo van a `sub_pin`.
- **En el padre:** se buscan las candidatas de cada instancia bajo los tipos que excluyen (`dnwell`), propios o de una hermana, con su punto interior, como hoy en `build_with`. Una candidata excluida pasa a ser una red común; una no excluida sigue candidata en el padre.
- **En la celda pedida** (la raíz de la extracción, D20.5), las que quedan se unen al sustrato.
- **`sub_pin`:** el de cada instancia se une al `sub_pin` del padre.

Así el pozo N profundo de la macro sobre las celdas de bit no obliga a aplanar.

### D20.5. La raíz y el aplanado (`mod.rs`, `flatten.rs`)

```rust
pub struct HierNets { pub root: Arc<CellNets>, memo: Arc<Memo>, /* la raíz cerrada: sustrato decidido */ }

pub fn extract(lib: &Library, cell: &Cell<'_>, rules: &DeviceRules, magic: Option<&MagInfo>) -> HierNets;

impl HierNets {
    /// La `Netlist` plana de hoy (para el visor, el LVS plano y el oráculo).
    /// Cuesta transformar pedazos y transistores, sin extraer nada.
    pub fn flatten(&self) -> Netlist;
    pub fn flat_size(&self) -> u64;           // pedazos + transistores aplanados, para el tope
    pub fn stats(&self) -> Stats;
}
```

**Cerrar la raíz.** Es lo único que depende de que la celda sea la raíz y no hija de otra: el sustrato (D20.4). Se aplica sobre una copia barata: las redes y la unión del sustrato. El resumen guardado no cambia.

**Cómo se aplana.** Se recorren las instancias recursivamente y se numeran las redes de cada `(camino, red)`. Las que `pins` une son una sola.

**Determinismo (NF3).** El orden de las redes y de los transistores no depende del paralelismo ni de la caché:
- los **transistores**, por posición (`y`, luego `x`, en la grilla de la librería);
- las **redes**, por su primer terminal en ese orden; después, las que solo tienen etiquetas, por etiqueta.

El orden no es el de la extracción plana de hoy. Nada depende de ese orden: los nombres `n<i>` y `X<i>` cambian una vez.

**El ejemplo `hier_check`** extrae una celda plana (`cell_nets`) y jerárquica (`extract` + `flatten`) y compara las dos según R20.2:
- **transistores:** se emparejan por (modelo, posición) y se comparan W y L;
- **redes:** de los terminales emparejados y las etiquetas sale una biyección entre las redes de un lado y las del otro; si una red de un lado va a dos del otro, falla;
- **nombres, pines y sustrato:** iguales en cada par.

Imprime la primera diferencia con su posición. La misma función (`same_netlist`) está en las pruebas.

### D20.6. Consultas registradas y corte temprano (`deps.rs`)

Toda consulta de un padre a una hija pasa por `query.rs`, que la anota en el `Deps` de esa instancia distinta. La anotación es `(Query, hash de la respuesta)`:

```rust
enum Query {
    Pieces { area: [i64; 4], types: TypeSet },   // pieces_in, en coordenadas enteras de la hija
    Point { at: (i64, i64), types: TypeSet },    // etiquetas, pines
    SubOpen { area: [i64; 4] },                  // candidatas al sustrato
    Context,                                     // la huella de contexto
}
```

**Hash de la respuesta.** Cubre las redes en su numeración estable y la geometría en enteros. No cubre los índices de instancia de más abajo: se normalizan a la posición, para que reordenar instancias no rompa el corte.

**Interfaz de la celda.** `iface` es el hash de:
- las redes con etiqueta de puerto;
- `sub_pin`;
- la huella de contexto;
- la caja de cada tipo.

Es lo mínimo que cualquier padre ve, pregunte lo que pregunte. Si cambia, el corte no se intenta (la vía rápida para "algo del borde cambió").

**Al armar `P`, para cada hija `h` que cambió de `NetKey`** respecto de una versión conocida de `P` (la otra versión de un diff, el commit anterior de un `log`, o el disco):

1. Si `iface(h_vieja) != iface(h_nueva)`, se rearma `P`.
2. Si son iguales, se repiten las consultas de `deps[h]` contra `h_nueva`:
   - **todas iguales:** `P` se reusa con la nueva `NetKey` de `h` en `instances` y en `deps`. La clave nueva de `P` apunta a ese resumen (un alias en `Memo`);
   - **alguna distinta:** se rearma `P`.

**Costo del corte.** Repetir las consultas es barato: son las mismas pocas cajas, ya podadas por `type_bbox`. En la matriz de bitcells son las de unas pocas vecindades, no las de 8 192 instancias.

**Cómo se encuentra "la versión conocida de `P`".** `Memo` guarda por **nombre de celda** el último resumen armado de cada lado. Solo es una pista: si la pista no corresponde, el corte no se intenta y no hay error. La validez la dan siempre las consultas repetidas, nunca el nombre.

**Prueba de oro.** Con corte y sin corte (`RIKU_NO_CUTOFF=1`), `flatten()` da lo mismo en cada commit de los demos y de una historia sintética:
- un contacto movido en el interior;
- uno movido sobre el borde;
- un corto interior entre dos puertos.

## D21. Memoria por huella (`key.rs`, `memo.rs`)

### Clave

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NetKey(pub u128);
```

Es un árbol de Merkle como `tree_prints`, calculado en la misma pasada. Las dos mitades de 64 bits salen de dos *seeds*, como `DiffCache::key`. La de una celda combina:

- la huella de su geometría propia (`CellTree::own`, todas las capas);
- sus etiquetas: capa, texto y origen cuantizado;
- sus puertos de Magic (`MagInfo`): nombre y rectángulo;
- por cada referencia, `(NetKey de la hija, transformación)`, ordenado;
- `rules.fingerprint` (D21.3), la unidad de la librería y `CARGO_PKG_VERSION` de `riku-mod-layout` (que sube a 0.5.0).

**Sin huella:** una celda sin huella jerárquica (`tree == None`) no tiene clave. Se extrae y no se guarda.

### Memoria del proceso

`Memo` es un `HashMap<NetKey, Arc<CellNets>>` global del proceso, con un tope en bytes estimados:
- **Estimación:** 32 B por punto de pedazo y 200 B por transistor.
- **Tope:** 512 MB por omisión; `RIKU_NETS_MEM_MB` lo cambia.
- **Al pasarse:** se saca primero lo menos usado.

Las vecindades (D20.3) y las comparaciones `(NetKey A, NetKey B) → Vec<Change>` (D22) van en otros mapas del mismo `Memo`, con su propio tope. Las comparaciones también van al disco (`riku/nets/cmp-<a>-<b>.json`): son chicas y valen entre corridas. Lo comparten:
- los dos lados de un diff;
- los commits de un `log`;
- los archivos de un `status`;
- el visor durante la sesión.

### Disco

**Dónde:** `riku/nets/` junto a `riku/diff/`. Los mismos `RIKU_CACHE_DIR`, `RIKU_NO_CACHE` y `--no-cache`.

**Qué:** un archivo `<clave>.json` por resumen. Los pedazos se guardan como enteros en la grilla de la librería, que pesan menos que los `f64` en JSON. No hay dependencias nuevas.

**Cuándo:**
- **Se guarda** solo un resumen que tardó ≥ 20 ms en armarse (sin contar sus hijas). Las celdas chicas se recalculan.
- **Se lee:** el resumen de una hija se lee del disco recién cuando hace falta (una consulta o un aplanado). Un padre leído del disco no carga a sus hijas.

**Tope:** 512 MB, aparte del de los diffs. Al escribir se borran las entradas más viejas (`prune` de `DiffCache`, que se generaliza a `DiffCache::nets()`).

**Corrupción:** una entrada ilegible se borra y se recalcula; nunca es un error.

### D21.3. La huella de las reglas

Al leer las reglas (`DeviceRules::parse`), se guarda el hash del texto del `.tech` o de la tabla compilada en `DeviceRules::fingerprint: u64`. Otro `.tech` instalado da otras claves (R21.4).

## D22. El diff, por celda (`compare.rs`)

```rust
/// Abiertos, cortos y renombres de `cell` entre dos versiones, a nivel de
/// esa celda (sus redes propias y las expuestas de sus hijas).
pub fn cell_net_changes_hier(a: &HierNets, b: &HierNets, cell: &str, unit_um: f64, changed: &[[f64; 4]]) -> Vec<NetChange>;
```

`gds_diff::net_changes` pasa a ser una **reconciliación** de los dos árboles (ver el modelo, «DOM virtual»):

```rust
/// Los cambios de la celda `a`→`b` y de todo lo distinto debajo. Memorizado por `(a, b)`.
fn reconcile(a: &CellNets, b: &CellNets, memo: &Memo) -> Arc<Vec<Change>>
```

1. **Qué se recorre.**
   - **El punto de partida:** las raíces de los dos lados (`hier::extract` de cada una).
   - **Qué se poda:** un par con la misma `NetKey` no se recorre, y un par ya comparado sale de `memo.compare`.
   - **Las hijas:** primero se emparejan las gemelas, por `NetKey` y transformación. Las que sobran, por nombre de celda y transformación. Se baja solo por los pares que difieren.
   - **Lo que queda sin pareja** (una instancia agregada o quitada) no se compara: es un cambio de geometría, que el XOR ya informa.
   - **La espina sucia** es exactamente lo que se recorre: la celda cambiada y sus ancestros. No hace falta el índice inverso para el diff (sí para el corte temprano, D20.6).

2. **Qué se recalcula.** Del lado B, solo la espina. Si el corte temprano la frenó en `P`, `P` y lo de arriba tienen el resumen reusado, y la comparación arriba de `P` lo detecta:
   - mismos pines, misma partición de las redes expuestas;
   - en la práctica, "sin cambios de redes" en un par de microsegundos.

3. **Anclas** de cada celda comparada:
   - **las de hoy:** las etiquetas propias por texto y los transistores propios emparejados por posición (`anchors`, `pair_devices`);
   - **instancias gemelas:** la misma `NetKey` y la misma transformación en los dos lados, emparejadas como multiconjunto, igual que `piece_keys`. Cada `NetRef` que aparece en `pins` de los dos lados une sus dos redes;
   - **instancias de una hija que cambió:** el mismo nombre y la misma transformación. Sus redes con etiqueta de puerto, por texto.

4. **Clasificación.** Las mismas reglas de hoy, separadas de cómo se arman las anclas:

   ```rust
   fn classify(cell: &str, a: &dyn NetSide, b: &dyn NetSide, anchors: &[(usize, usize)], unit_um: f64, changed: &[[f64; 4]]) -> Vec<NetChange>
   ```

   - grupos con varias redes de A y una de B: un **corto**;
   - una de A y varias de B: un **abierto**, o **separado** si cada red de B se queda con alguna etiqueta del corto;
   - una y una con otra etiqueta: un **renombre**.

   `net_changes` (plana) pasa a llamar a `classify`, así que las pruebas de `nets/diff.rs` siguen valiendo para las dos.

5. **La celda más baja.** Un corto se informa solo en la celda más baja donde aparece: la regla `below` de hoy. Con la reconciliación sale sola: un nivel recibe los cambios de sus hijas, y no repite un corto cuyas redes ya vienen de un cambio de una hija.

6. **Del *patch* al informe.**
   - **Qué se memoriza:** `Change` es lo de una celda en sus propias coordenadas, `(cell, NetChange | DeviceChange)`. Así se puede reusar en cualquier padre.
   - **Qué hace `gds_diff`:** lo pasa a `report.nets` y a `report.devices` como hoy, una vez por celda, sin multiplicar por instancias.
   - **Por qué así:** el informe de hoy ya es por celda, no por instancia.

**Nombres de redes (R22.5).** `NetSide::label(i)` da, en orden:
- la etiqueta;
- el terminal de un transistor propio, como hoy;
- la hija y su red: `<celda>@(x, y)/<etiqueta o terminal de la hija>`, con el origen de la instancia en µm.

**Transistores (R22.3).** `device_changes` compara en cada celda con un cambio propio los transistores de `CellNets::devices`, los propios, más los de instancias aplanadas por contexto. Ya no aplana la celda entera.

**Qué se borra.**
- **Se va:** `nets::context`, `cell_nets_in` con ventanas, `pieces_changed`, la confirmación con la celda entera y los avisos "comparadas solo cerca del cambio" y "más de 2 millones de polígonos".
- **Queda detrás de `RIKU_FLAT_NETS=1`** (NF5) mientras dura la ronda: el código plano.
- **En C3 se decide** si `cell_nets_in` con ventanas se borra ya o en la ronda 6.

**El visor de un diff** (`viewer_core_compat`, la celda abierta) usa `cell_net_changes_hier` y los transistores propios. Mismas marcas y mismos ítems.

## D23. Visor y LVS

**Visor (`add_electrical`).** `hier::extract`, y luego:
- **`flat_size() ≤ MAX_POLYGONS`:** `flatten()` y todo sigue como hoy (capa Transistores, `LayoutNets`, resumen).
- **Por encima:** solo el resumen con `stats()`, que dice cuántos transistores y redes hay, y el aviso de hoy, que pasa a decir "abrí una sub-celda para verlos" (R23.2).

Como el resumen está en `Memo`, abrir otra vez la celda o un padre no extrae de nuevo (R23.1).

**LVS (`layout_spice`).**
- **`flat_size() ≤ MAX_POLYGONS`:** `spice(flatten())`, la plana de hoy. `LayoutNets` de la misma `flatten()` ubica los nombres (ronda 2, sin cambios).
- **Por encima:** `hier::spice`.

**`hier::spice`:**

| Qué | Cómo |
|---|---|
| Qué celdas | Un `.subckt` por celda con transistores en su sub-jerarquía, de abajo hacia arriba y en orden de nombre |
| Las que solo llevan cables | Se meten en su padre: sus `pins` ya dicen qué redes unen |
| Pines de un `.subckt` | Las redes con etiqueta de puerto y las expuestas (las que algún padre usa en `pins`), juntando todos los usos de esa celda en la extracción, en orden de nombre |
| Pines que un uso no conecta | Una red nueva en el padre |
| Nombres | Los de hoy: `X<i>` por transistor dentro de su celda, `X<celda>_<i>` por instancia, `n<i>` para las redes sin nombre |

**Netgen** compara por niveles: aparea `.subckt` del mismo nombre y aplana lo que no tiene pareja.

**`parse_netgen`** hoy lee solo la última entrada del `comp.json`. Pasa a leer todas:
- el **veredicto**, de la de arriba (la última), como hoy;
- las **discrepancias**, de todas, con el camino de la celda delante del nombre (`bank/X12`).

Ubicar esas en el visor queda fuera de alcance.

**R23.4** se comprueba con:
- los demos `ota` e `inversor` (`riku lvs` y `log --lvs -f json`), iguales antes y después;
- las pruebas de `lvs.rs`, más una con un `comp.json` de dos niveles.

## D24. Verificación y medidas

| Qué | Cómo | Dónde |
|---|---|---|
| Plana = jerárquica | `same_netlist` (D20.5) en pruebas: el inversor (Magic, 2 niveles), la SRAM de `examples/GDS/` (GDS, varios niveles, `dnwell`), una matriz sintética con AREF y espejos | `cargo test` |
| Ídem con el PDK | `hier_check` sobre las 437 `sky130_fd_sc_hd` y las celdas de `tools/verify/devices/` y `nets/` (tres PDK); los demos `ota`, `sram`, `inversor`, `chip` (cada celda de la macro que hoy cabe) | `tools/verify/nets/hier.sh` |
| Contra Netgen | La SPICE plana de hoy contra la de `flatten`, de cada celda de arriba: "coinciden" | `tools/verify/nets/hier.sh` |
| Un chip entero contra Magic (R24.2) | La macro `sky130_sram_1kbyte_1rw1r_32x256_8`: `hier::spice` contra Magic `gds read` + `extract all` + `ext2spice lvs`, con Netgen y el `setup.tcl` de SKY130 | `tools/verify/nets/chip_vs_magic.sh` |
| Diff | Las pruebas de `gds_diff.rs` y `nets/diff.rs` (corto en el padre, abierto sobre instancia sin cambios, el más bajo); los demos `inversor` (`check_show`, `check_lvs`) y `sram` | `cargo test`, `python3 tools/demos/inversor.py` |
| Contexto | Pruebas sintéticas: un implante del padre sobre la difusión de la hija, un poly del padre sobre la difusión de la hija, un `dnwell` del padre sobre celdas con `pwell` → el mismo resultado que la plana, y `stats` cuenta lo aplanado (salvo el `dnwell`, que no aplana) | `cargo test` |
| Caché | Mismo resultado saliendo de memoria, del disco o recalculado; entrada corrupta; otra `rules.fingerprint` → otra clave | `cargo test` |
| Corte temprano | Una historia sintética (contacto interior, contacto en el borde, corto interior entre dos puertos) y los demos: con y sin `RIKU_NO_CUTOFF=1`, el mismo `flatten()` y el mismo diff. `stats` cuenta los cortes; el contacto interior de un bitcell no rearma `array` | `cargo test` |
| Reconciliación | `reconcile` con memoria de comparaciones da lo mismo que sin ella; `diff v1.0 HEAD` después de `log` reusa pares (`RIKU_PROFILE`) | `cargo test`, a mano |
| Tiempos y memoria | Los casos de R24.3 con `RIKU_PROFILE=1` y `/usr/bin/time -v`, antes (T20.1) y después | a mano, en `/tmp` del contenedor |

## Riesgos

| Riesgo | Qué se hace |
|---|---|
| Reglas `cifinput` no locales (`grow`/`shrink` que cruzan el borde de una celda) dan otra región que la plana | Lo detecta `hier_check`. Si pasa, el par de capas entra en `context_pairs` con el alcance del `grow`, y esa instancia se aplana |
| La prueba gruesa de contexto da positivo de más y se aplana demasiado (se pierde el ahorro) | `stats` lo cuenta y `RIKU_PROFILE` lo imprime. Si pasa en la macro, se refina la huella de contexto (más cajas) o se usa la prueba fina |
| Una celda que cubre a todas (rejilla de alimentación) da una vecindad distinta por instancia | Se mide en la macro. Si pesa, para esas vecindades se piden solo los pedazos de la hija chica dentro de la caja de la grande |
| `pins` de la raíz de un chip con millones de redes expuestas | `NetRef` con `path` corto (`Vec<u32>`) y `pins` ordenado (búsqueda binaria). Medir la memoria (R24.3: no más de +20 %) |
| Un resumen grande en JSON pesa en disco | Coordenadas enteras; solo los que tardan ≥ 20 ms; tope de 512 MB aparte |
| Netgen con una SPICE jerárquica y un esquemático plano | Netgen aplana lo que no tiene pareja. Se prueba con el chip contra Magic (que también escribe jerárquico) |
| El corte temprano reusa un padre que no debía (una consulta que no se registró) | Toda lectura de una hija pasa por `query.rs`, que es lo único que registra. La prueba de oro con y sin corte. `RIKU_NO_CUTOFF=1` lo apaga |
| La numeración estable no alcanza y el corte casi nunca pega | `stats` lo mide en el demo `chip`. Sin corte, el diseño sigue siendo correcto: solo rearma la espina, que ya es barato con las vecindades memorizadas |
| La ronda es grande (L) | Por etapas que se pueden cortar: T21–T22 (extracción y oráculo) ya sirven solas; la caché (T23), el diff (T24) y el visor/LVS (T25) se apoyan en ellas. Si algo no llega, queda en `pendientes.md` con lo medido |
