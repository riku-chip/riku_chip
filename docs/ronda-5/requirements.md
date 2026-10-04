# Ronda 5: extracción jerárquica con memoria por huella (requisitos)

Las redes y los transistores de un layout se extraen **celda por celda, una vez por contenido**: una celda que no cambió no se vuelve a analizar, y su padre solo une lo que toca a sus hijas (como KLayout y Magic). Diseño en [`design.md`](design.md), pasos en [`tasks.md`](tasks.md).

## Contexto (revisado el 2026-10-04 sobre `main`, `4b6c5e9`)

**Cómo se extrae hoy.** `nets::cell_nets` aplana la celda pedida (todas sus sub-celdas) en las capas que importan y arma las redes con `extract::build`:

1. la región de cada tipo (en un GDS, con las reglas `cifinput`; en Magic, la capa ya es el tipo);
2. los pedazos y sus uniones (*union-find*);
3. el sustrato;
4. los terminales de cada transistor;
5. las etiquetas de la propia celda.

Tope: `devices::MAX_POLYGONS` = 2 millones de polígonos aplanados. Más que eso no se analiza: hay un aviso y se analizan las sub-celdas.

**Quién la usa.** Cada uso vuelve a aplanar y a extraer; nada se guarda entre llamadas:

| Uso | Dónde | Qué hace |
|---|---|---|
| Diff | `gds_diff::net_changes` y `device_changes` | Cada celda con un cambio propio se extrae entera, en los dos lados. Sus ancestros se miran solo en **ventanas** alrededor del cambio (`nets::context`) y, si ahí aparece algo, se **confirma con la celda entera** |
| Visor, un layout | `viewer_core_compat::add_electrical` | La capa Transistores, la sonda de redes (`LayoutNets`) y el resumen |
| Visor, un diff | `viewer_core_compat`, la celda abierta | `cell_net_changes` y `cell_device_changes` |
| LVS | `nets::layout_spice` → `riku lvs`, `log --lvs`, `status --lvs` | Netlist plana, un `X<i>` por *finger*; el visor ubica cada nombre (`net_named`, `device_named`) |

**Lo que ya existe y sirve.**
- **Huella jerárquica** (`prints::tree_prints`): un árbol de Merkle por celda (geometría propia, y por cada referencia, la huella de la hija y su transformación). Dos celdas con la misma huella aplanan a lo mismo.
- **Caché en disco** de resultados del diff (`DiffCache`).

**Lo que cuesta** (ronda 4, binario de release, 12 núcleos):

| Caso | Tiempo |
|---|---|
| Demo `chip`, el commit del bitcell: primer `riku show` | 9,4 s (~2 s de XOR; el resto, redes de sub-celdas y la confirmación en los ancestros) |
| Demo `chip`: `riku log -n 10` | 7,4 s |
| Demo `chip`: `riku diff v1.0 HEAD` | 7,1 s |
| SRAM de `examples/`: un commit con relleno en la celda de arriba | ~1,7 s (las redes de la macro entera, en los dos lados) |

**Lo que no se puede hoy.**
- Las redes y el LVS de una celda de más de 2 millones de polígonos (un chip entero).
- Reusar la extracción de una celda entre commits, entre archivos o entre corridas.

## R20. Extracción por celda

- **R20.1** Cada celda se extrae **sola**: su geometría propia y, de cada instancia, un resumen de la hija ya extraída (sus redes, sus pedazos y sus pines). El padre agrega sus transistores propios y las uniones entre lo suyo y sus hijas, y entre hijas que se tocan o se superponen.
- **R20.2** **Mismo resultado que hoy.** Aplanada, la extracción jerárquica da la misma netlist que la plana actual, en todas las celdas que hoy se pueden analizar:
  - los mismos transistores (modelo, W, L y posición);
  - las mismas redes (qué terminales y qué etiquetas une cada una);
  - los mismos nombres y pines;
  - el mismo sustrato.

  Se comprueba con:
  - las 437 celdas `sky130_fd_sc_hd`;
  - las de `tools/verify/devices/` y `tools/verify/nets/` (SKY130, GF180, IHP);
  - la SRAM de `examples/GDS/`;
  - los demos `ota`, `sram`, `inversor` y `chip`;
  - Netgen comparando la netlist plana de hoy con la nueva: "coinciden".
- **R20.3** **Contexto.** Cuando lo que dibuja el padre o una hija vecina cambia lo que se extrae dentro de una hija, esa instancia se analiza aplanada dentro del padre, como hoy:
  - un implante o un pozo que el padre dibuja sobre la difusión de la hija;
  - un poly del padre sobre la difusión de la hija (un transistor entre dos niveles);
  - el pozo N profundo de la macro sobre el pozo P de las celdas.

  Nunca se da un resultado distinto del plano por ahorrar tiempo. `RIKU_PROFILE=1` dice cuántas instancias se aplanaron y por qué.
- **R20.4** Sin tope de polígonos para extraer: una celda de cualquier tamaño se extrae si sus hijas caben de a una. El tope queda solo donde hace falta todo aplanado a la vez (R23.2).
- **R20.5** Las celdas iguales se extraen una vez, y las interacciones iguales se calculan una vez. Una interacción es igual si coinciden la huella de las dos hijas y la transformación de una respecto de la otra (las 8 192 instancias de un bitcell en una matriz tienen unas pocas vecindades distintas).

## R21. Memoria por huella

- **R21.1** La extracción de cada celda se guarda con la **huella de lo que la determina**:
  - la huella jerárquica de su geometría;
  - sus etiquetas y las de sus hijas;
  - los puertos de Magic;
  - las reglas del PDK;
  - la unidad;
  - la versión de `riku-mod-layout`.

  Nunca se guarda por nombre ni por fecha. Una celda con la misma huella, en otro commit, en otro archivo o en el otro lado de un diff, no se vuelve a extraer.
- **R21.2** Se guarda **en memoria** durante el proceso (un `log` de muchos commits, los dos lados de un diff, el visor que abre otra celda) y **en disco**, junto a la caché de diffs. En disco va solo lo que vale la pena: lo que tardó en extraerse más que lo que tarda en leerse.
- **R21.3** Topes de memoria y de disco, y se desactiva igual que la caché de diffs (`RIKU_NO_CACHE=1`, `--no-cache`). Una entrada ilegible se borra y se recalcula; nunca es un error.
- **R21.4** Un cambio en las reglas del PDK instalado (otro `.tech`) invalida lo guardado con las anteriores.
- **R21.5** **Corte temprano.** Un padre depende solo de lo que le pregunta a cada hija. Si la hija cambia pero sus respuestas son las mismas, el padre no se rearma y la propagación hacia la raíz se corta ahí. Ejemplo: un contacto movido en el interior de un bitcell no rearma la matriz, el banco ni la macro. El resultado es el mismo que sin corte (se puede apagar con `RIKU_NO_CUTOFF=1` para comprobarlo).

## R22. El diff, por celda

- **R22.1** **Abiertos, cortos y renombres** se comparan por celda con la extracción jerárquica de los dos lados:
  - en la celda que cambió, como hoy;
  - en cada ancestro, con lo que cambió (lo propio y lo que le llega de sus hijas) y las anclas de siempre (etiquetas y transistores);
  - además, como ancla, cada instancia gemela (la misma hija con la misma transformación en los dos lados): sus redes son las mismas en A y en B.

  Reemplaza a las ventanas y a la confirmación con la celda entera (`nets::context`).
- **R22.2** **Mismos hallazgos que hoy** en los casos que hoy se ven, comprobados con las pruebas de `gds_diff.rs` y `nets/diff.rs`:
  - el corto que aparece recién en el padre;
  - el abierto sobre una instancia sin cambios;
  - el corto que se informa en la celda más baja;
  - los demos `inversor` y `sram`.

  Sin el aviso "redes comparadas solo cerca del cambio": ya no hace falta.
- **R22.3** **Transistores:** se comparan los propios de cada celda que cambió. Los de una sub-celda se comparan en ella y no se repiten en cada ancestro, salvo los de una instancia aplanada por contexto (R20.3).
- **R22.4** **Celdas grandes:** sin el aviso "no comparadas en … (más de 2 millones de polígonos)": un cambio en la celda de arriba de un chip se compara.
- **R22.5** **Nombres** de una red sin etiqueta en la celda donde se informa: por la hija y la red dentro de ella (`sram_bank@(12.0, 40.5)/bl3`), además de como hoy (el terminal de un transistor propio).
- **R22.6** **Reconciliación.** La comparación recorre los dos árboles a la vez y baja solo por los pares de celdas distintos (las gemelas, por su huella; el resto, por nombre y posición). El resultado de comparar dos versiones de una celda se guarda por el par de huellas: `log`, `show` y `diff` de rangos que se superponen no repiten la comparación.

## R23. Visor y LVS

- **R23.1** **El visor de un layout** usa la extracción guardada. Abrir otra vez la misma celda, o una celda que contiene otras ya abiertas, no vuelve a extraer lo que ya se extrajo. Mismo contenido que hoy: capa Transistores, sonda de redes, resumen.
- **R23.2** **Celdas grandes en el visor.** Por encima de 2 millones de polígonos, el visor sigue sin la capa Transistores ni la sonda de redes (hacen falta aplanadas para dibujarlas). El resumen sí dice cuántos transistores y redes tiene, por la extracción jerárquica. Mostrarlas jerárquicamente queda para después.
- **R23.3** **LVS.**
  - **Hasta el tope:** la netlist del layout se escribe **plana** como hoy, aplanando la extracción jerárquica: mismos nombres, así el cruce con el visor (ronda 2) no cambia.
  - **Por encima del tope:** se escribe **jerárquica**, un `.subckt` por celda con transistores. Las celdas que solo llevan cables (vías, contactos) se meten en su padre. Netgen compara por niveles y aplana lo que no tenga pareja en el esquemático.

  En un LVS jerárquico, una discrepancia dentro de una sub-celda se muestra con su camino (`bank/X12`). Ubicarla en el visor queda para después.
- **R23.4** El resultado del LVS de un par que hoy ya se puede analizar no cambia: el veredicto de los demos `ota` e `inversor`, y `log --lvs`.

## R24. Verificación y medidas

- **R24.1** **Oráculo de la ronda:** un ejemplo (`examples/hier_check.rs`) que extrae una celda de las dos maneras, plana y jerárquica, y compara las dos netlists según R20.2. Lo corren:
  - las pruebas, con las celdas del repo;
  - `tools/verify/nets/`, con las del PDK.
- **R24.2** **Un chip entero contra Magic.** La macro `sky130_sram_1kbyte_1rw1r_32x256_8` del demo `chip` se extrae entera con Riku y con Magic (`extract all` + `ext2spice`), y Netgen compara las dos netlists. Hoy Riku no puede extraerla.
- **R24.3** **Tiempos**, antes y después, con el binario de release:

  | Caso | Antes | Meta |
  |---|---|---|
  | Demo `chip`: primer `riku show` del commit del bitcell | 9,4 s | ≤ 4 s |
  | Demo `chip`: `riku log -n 10`, sin caché | 7,4 s | ≤ 4 s |
  | Demo `chip`: `riku diff v1.0 HEAD` | 7,1 s | baja |
  | Un `log` con la caché de redes de una corrida anterior | — | solo lo que cambió |
  | Demo `chip`: `riku diff v1.0 HEAD` justo después de `log` | — | reusa las comparaciones |
  | Un contacto movido en el interior del bitcell | — | sin rearmar la matriz ni lo de arriba |
  | SRAM de `examples/`: el commit de relleno en la celda de arriba | 1,7 s | ≤ 0,5 s |
  | Visor: abrir la SRAM de `examples/` dos veces | — | la segunda, sin extraer |
  | Memoria pico | — | no sube más de un 20 % en ningún caso |

  Si una meta no se alcanza, se anota cuánto se llegó y por qué. Los números quedan en `docs/desarrollo.md` (Rendimiento) y en el README del demo `chip`.

## No funcionales

- **NF1** Sin cambios en el crate de Carlos, ni en el contrato de `viewer-core`.
- **NF2** Sin dependencias nuevas.
- **NF3** Determinista: la misma entrada da la misma netlist (el mismo orden de redes y transistores), corra en paralelo o no, salga de la caché o no.
- **NF4** Los JSON públicos no cambian de esquema (`riku-diff`, `riku-log`, `riku-lvs`). Los avisos que desaparecen se documentan.
- **NF5** Se puede volver a la extracción plana con `RIKU_FLAT_NETS=1`, para comparar y como salida de emergencia mientras la jerárquica se asienta. Se quita en una ronda posterior.

## Fuera de alcance

- Transistores y redes de una celda de más de 2 millones de polígonos **dibujados** en el visor (R23.2). Para eso hace falta una sonda jerárquica.
- Ubicar en el visor una discrepancia del LVS jerárquico dentro de una sub-celda (R23.3).
- Diodos, capacitores y resistores en el diff (siguen como hoy).
- Extracción de parásitos.
