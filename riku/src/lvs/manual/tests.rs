use super::*;
use std::collections::{BTreeSet};

const N: &str = "sky130_fd_pr__nfet_01v8";
const P: &str = "sky130_fd_pr__pfet_01v8";

fn sd(name: &str, model: &str, pins: [&str; 4], w: f64) -> SchDevice {
    SchDevice { name: name.into(), model: model.into(), pins: pins.map(str::to_string), w: Some(w), l: Some(0.5), m: 1.0 }
}

fn ld(model: &str, at: (f64, f64), pins: [&str; 4], w: f64) -> LayDevice {
    LayDevice { model: model.into(), at, gate: [at.0 - 0.25, at.1 - 0.5, at.0 + 0.25, at.1 + 0.5], cell: None, local: at, w, l: 0.5, pins: pins.map(str::to_string) }
}

/// Un inversor con la salida por un buffer: M1/M2 el inversor, M3 un
/// nfet que lleva `out` a `x`.
fn sides() -> (Vec<SchDevice>, Vec<LayDevice>) {
    let sch = vec![
        sd("M1", N, ["out", "in", "VSS", "VSS"], 1.0),
        sd("M2", P, ["out", "in", "VDD", "VDD"], 2.0),
        sd("M3", N, ["x", "out", "VSS", "VSS"], 1.0),
    ];
    let lay = vec![
        ld(N, (1.0, 1.0), ["n1", "in", "VSS", "VSS"], 1.0),
        // M2 en dos dedos.
        ld(P, (1.0, 5.0), ["VDD", "in", "n1", "VDD"], 1.0),
        ld(P, (2.0, 5.0), ["n1", "in", "VDD", "VDD"], 1.0),
        ld(N, (4.0, 1.0), ["x", "n1", "VSS", "VSS"], 1.0),
    ];
    (sch, lay)
}

fn bind(s: &str, refs: &[(&str, f64, f64)]) -> Bind {
    Bind { schematic: s.into(), layout: refs.iter().map(|&(m, x, y)| LayoutRef { model: m.into(), at: [x, y], cell: None, local: None }).collect() }
}

fn full() -> MapFile {
    let mut m = MapFile::new("a.sch", "a.gds", None);
    m.binds = vec![
        bind("M1", &[(N, 1.0, 1.0)]),
        bind("M2", &[(P, 1.0, 5.0), (P, 2.0, 5.0)]),
        bind("M3", &[(N, 4.0, 1.0)]),
    ];
    m
}

#[test]
fn todo_vinculado_y_coherente_es_limpio() {
    let (sch, lay) = sides();
    let c = check(&full(), &sch, &lay);
    assert!(c.clean(), "{c:#?}");
    assert_eq!(c.nets["out"], BTreeSet::from(["n1".to_string()]));
}

#[test]
fn fuente_y_drenaje_se_deciden_por_lo_vinculado() {
    // M2 primero: su primer dedo tiene la fuente y el drenaje al revés
    // que el esquemático, y todavía no hay nada vinculado. No debe
    // inventar un corto out–VDD.
    let (sch, lay) = sides();
    let mut m = full();
    m.binds.rotate_left(1);
    let c = check(&m, &sch, &lay);
    assert!(c.clean(), "{c:#?}");
}

#[test]
fn avanza_de_a_poco() {
    let (sch, lay) = sides();
    let mut m = full();
    m.binds.truncate(1);
    let c = check(&m, &sch, &lay);
    assert_eq!(c.unbound_schematic, ["M2", "M3"]);
    assert_eq!(c.unbound_layout.len(), 3);
    assert!(c.shorts.is_empty() && c.opens.is_empty() && c.params.is_empty());
}

#[test]
fn un_corto_y_un_ancho_se_ven_con_los_vinculos() {
    let (sch, mut lay) = sides();
    // En el layout, la compuerta de M3 quedó en `in` (corto out–in) y
    // M1 es más angosto.
    lay[3].pins[G] = "in".into();
    lay[0].w = 0.5;
    let c = check(&full(), &sch, &lay);
    assert_eq!(c.shorts, [("in".to_string(), vec!["in".to_string(), "out".to_string()])]);
    assert_eq!(c.params, [("M1".to_string(), "W 1 ≠ 0.5".to_string())]);
}

#[test]
fn mover_todo_dentro_de_la_celda_se_reubica_solo() {
    let (sch, lay) = sides();
    // Todo girado 90° y corrido (10, 20).
    let moved: Vec<LayDevice> = lay
        .iter()
        .map(|d| {
            let p = orient(d.at, 1);
            LayDevice { at: (round3(p.0 + 10.0), round3(p.1 + 20.0)), ..d.clone() }
        })
        .collect();
    let c = check(&full(), &sch, &moved);
    assert!(c.clean(), "{c:#?}");
    let m = c.moved.unwrap();
    assert_eq!((m.orient, m.count), (1, 4));
    assert_eq!(c.updated.binds[0].layout[0].at, [9.0, 21.0]);
}

#[test]
fn uno_movido_solo_se_encuentra_por_conectividad() {
    let (sch, mut lay) = sides();
    lay[3].at = (40.0, 7.0);
    let c = check(&full(), &sch, &lay);
    assert!(c.clean(), "{c:#?}");
    assert_eq!(c.by_connectivity, ["M3"]);
}

#[test]
fn sugiere_desde_los_pines_sin_adivinar() {
    let (sch, lay) = sides();
    let got = suggest(&MapFile::new("a.sch", "a.gds", None), &sch, &lay);
    let names: Vec<&str> = got.iter().map(|b| b.schematic.as_str()).collect();
    assert_eq!(names.len(), 3, "{got:#?}");
    let m2 = got.iter().find(|b| b.schematic == "M2").unwrap();
    assert_eq!(m2.layout.len(), 2, "los dos dedos juntos");
    // Con lo sugerido, todo queda limpio.
    let mut m = MapFile::new("a.sch", "a.gds", None);
    m.binds = got;
    assert!(check(&m, &sch, &lay).clean());
}

#[test]
fn vincular_reemplaza_lo_anterior_de_los_dos_lados() {
    let (sch, lay) = sides();
    let mut m = full();
    // M3 pasa a ser el primer dedo de M2: M2 se queda con el otro.
    super::bind(&mut m, "M3", &[1], &lay);
    let m3 = m.binds.iter().find(|b| b.schematic == "M3").unwrap();
    assert_eq!(m3.layout.len(), 1);
    assert_eq!(m3.layout[0].at, [1.0, 5.0]);
    assert_eq!(m.binds.iter().find(|b| b.schematic == "M2").unwrap().layout.len(), 1);
    // Y lo que se deduce lo dice: modelo distinto en M3.
    assert!(!check(&m, &sch, &lay).models.is_empty());
    unbind(&mut m, "M3");
    assert!(m.binds.iter().all(|b| b.schematic != "M3"));
}

#[test]
fn el_historial_marca_cortos_y_avance() {
    let base = Summary { bound: 5, total: 9, shorts: 0, opens: 0, clean: false, ..Summary::default() };
    let worse = Summary { shorts: 1, ..base.clone() };
    assert_eq!(transitions(&base, &worse), [Transition::NewShort]);
    assert_eq!(transitions(&worse, &base), [Transition::ShortFixed]);
    let done = Summary { bound: 9, clean: true, ..base.clone() };
    assert_eq!(transitions(&base, &done), [Transition::Clean, Transition::Linked(4)]);
    assert_eq!(transitions(&done, &Summary { differences: 2, clean: false, ..done.clone() }), [Transition::Broke]);
}

/// Seis transistores en fila (uno por columna, cada uno con sus redes).
fn row(n: usize, y: f64) -> (Vec<SchDevice>, Vec<LayDevice>, MapFile) {
    let pins = |i: usize| [format!("d{i}"), format!("g{i}"), format!("s{i}"), "VSS".to_string()];
    let sch = (0..n).map(|i| SchDevice { name: format!("M{i}"), model: N.into(), pins: pins(i), w: Some(1.0), l: Some(0.5), m: 1.0 }).collect();
    let lay = (0..n).map(|i| LayDevice { model: N.into(), at: (i as f64, y), gate: [0.0; 4], cell: None, local: (i as f64, y), w: 1.0, l: 0.5, pins: pins(i) }).collect();
    let mut m = MapFile::new("a.sch", "a.gds", None);
    m.binds = (0..n).map(|i| Bind { schematic: format!("M{i}"), layout: vec![LayoutRef { model: N.into(), at: [i as f64, 0.0], cell: None, local: None }] }).collect();
    (sch, lay, m)
}

#[test]
fn en_un_arreglo_regular_gana_el_movimiento_que_no_contradice() {
    // Todo subió 10: correrlo además "un dedo" también encaja 5 de 6,
    // pero vincularía cada transistor con las redes del vecino.
    let (sch, lay, m) = row(6, 10.0);
    let c = check(&m, &sch, &lay);
    let mv = c.moved.expect("movido");
    assert_eq!((mv.dx, mv.dy, mv.count), (0.0, 10.0, 6), "{c:#?}");
    assert!(c.shorts.is_empty() && c.opens.is_empty(), "{c:#?}");
}

#[test]
fn un_movimiento_ambiguo_no_reubica_nada() {
    // Dos transistores iguales (mismas redes) a 2 µm, y en el layout
    // tres dedos a 2 µm: correr 10 o 12 encaja igual. No se elige.
    let sch: Vec<SchDevice> = ["M1", "M2"].iter().map(|n| sd(n, N, ["d", "g", "s", "VSS"], 1.0)).collect();
    let lay: Vec<LayDevice> = [10.0, 12.0, 14.0].iter().map(|&x| ld(N, (x, 0.0), ["d", "g", "s", "VSS"], 1.0)).collect();
    let mut m = MapFile::new("a.sch", "a.gds", None);
    m.binds = vec![bind("M1", &[(N, 0.0, 0.0)]), bind("M2", &[(N, 2.0, 0.0)])];
    let c = check(&m, &sch, &lay);
    assert!(c.moved.is_none() && c.moved_ambiguous, "{c:#?}");
    assert_eq!(c.lost.len(), 2);
}

#[test]
fn en_un_layout_simetrico_gana_el_movimiento_que_respeta_los_nombres() {
    // M1 (compuerta A) y M2 (compuerta B), y el layout girado 90° y
    // corrido. El layout es simétrico: girarlo al revés también encaja los
    // dos dedos, pero cambia A por B.
    let sch = vec![sd("M1", N, ["d", "A", "s", "VSS"], 1.0), sd("M2", N, ["d", "B", "s", "VSS"], 1.0)];
    let lay = vec![ld(N, (10.0, 0.0), ["d", "A", "s", "VSS"], 1.0), ld(N, (10.0, 2.0), ["d", "B", "s", "VSS"], 1.0)];
    let mut m = MapFile::new("a.sch", "a.gds", None);
    m.binds = vec![bind("M1", &[(N, 0.0, 0.0)]), bind("M2", &[(N, 2.0, 0.0)])];
    let c = check(&m, &sch, &lay);
    assert!(!c.moved_ambiguous, "{c:#?}");
    assert_eq!(c.moved.map(|m| m.orient), Some(1), "{c:#?}");
    assert!(c.clean(), "{c:#?}");
}

#[test]
fn los_pines_se_comparan_por_las_redes_vinculadas() {
    let (sch, lay) = sides();
    let mut c = check(&full(), &sch, &lay);
    // `in` del esquemático es `in` en el layout; `out` no tiene pin en
    // el layout; `x` es pin del layout pero llega a otra red; `EN` sobra.
    let sch_ports: Vec<String> = ["in", "out", "x"].map(str::to_string).to_vec();
    let lay_ports: Vec<String> = ["in", "x", "EN"].map(str::to_string).to_vec();
    c.nets.insert("x".into(), BTreeSet::from(["n9".to_string()]));
    c.pins = check_pins(&c, &sch_ports, &lay_ports);
    let names: Vec<&str> = c.pins.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(names, ["out", "x", "EN"]);
    assert!(!c.clean());
}

#[test]
fn limpio_no_es_completo_si_hay_algo_sin_revisar() {
    let (sch, lay) = sides();
    let mut c = check(&full(), &sch, &lay);
    assert!(c.clean() && c.complete());
    c.unchecked = vec!["R1".into()];
    assert!(c.clean() && !c.complete());
}

#[test]
fn lo_que_no_es_transistor_queda_sin_revisar() {
    let spice = ".subckt t in out VDD\nXM1 out in VSS VSS sky130_fd_pr__nfet_01v8 L=0.5 W=1\nR1 a b 1k\nx2 a b amp W=2\n* nota\n.ends\n";
    let (ports, others) = schematic_extras(spice, "t", &|n| n.strip_prefix('X').map(str::to_string));
    assert_eq!(ports, ["in", "out", "VDD"]);
    assert_eq!(others, ["R1", "x2 (amp)"]);
}

#[test]
fn una_instancia_movida_se_reencuentra_por_su_celda() {
    // Dos inversores iguales (celda `inv`), cada uno con un transistor en
    // (1, 1) de la celda. La instancia de M2 se movió: su dedo no está
    // donde estaba, pero es el único de `inv` en (1, 1) que queda libre.
    let sch = vec![sd("M1", N, ["a", "x", "VSS", "VSS"], 1.0), sd("M2", N, ["b", "y", "VSS", "VSS"], 1.0)];
    let dev = |at: (f64, f64), pins: [&str; 4]| LayDevice { cell: Some("inv".into()), local: (1.0, 1.0), ..ld(N, at, pins, 1.0) };
    let lay = vec![dev((11.0, 1.0), ["a", "x", "VSS", "VSS"]), dev((51.0, 31.0), ["b", "y", "VSS", "VSS"])];
    let r = |x: f64, y: f64| LayoutRef { model: N.into(), at: [x, y], cell: Some("inv".into()), local: Some([1.0, 1.0]) };
    let mut m = MapFile::new("a.sch", "a.gds", None);
    m.binds = vec![Bind { schematic: "M1".into(), layout: vec![r(11.0, 1.0)] }, Bind { schematic: "M2".into(), layout: vec![r(21.0, 1.0)] }];
    let c = check(&m, &sch, &lay);
    assert!(c.clean(), "{c:#?}");
    assert_eq!(c.by_cell, ["M2"]);
    assert_eq!(c.updated.binds[1].layout[0].at, [51.0, 31.0]);
    // Y el archivo lo guarda con su celda.
    assert!(c.updated.to_text().contains("cell = \"inv\", local = [1.000, 1.000]"), "{}", c.updated.to_text());
}

#[test]
fn el_archivo_se_escribe_entero_y_sin_temporales() {
    let dir = std::env::temp_dir().join(format!("riku-lvs-write-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("lvs").join("a.toml");
    let m = full();
    m.write(&path).unwrap();
    // Otra vez encima (el renombre reemplaza al que había).
    m.write(&path).unwrap();
    let back = MapFile::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let left: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().flatten().map(|e| e.file_name()).collect();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(back, m);
    assert_eq!(left.len(), 1, "{left:?}");
}

#[test]
fn el_esquematico_se_aplana_con_rutas_y_parametros() {
    let spice = ".subckt top a b\nXM1 a b VSS VSS sky130_fd_pr__nfet_01v8 L=0.5 W=1\nx1 b c inv W=2\nR9 a c 1k\n.ends\n\
.subckt inv in out W=1\nXM2 out in mid VSS sky130_fd_pr__nfet_01v8 L=0.5 W=W\nx2 mid out buf\n.ends\n\
.subckt buf p q\nXM3 q p VSS VSS sky130_fd_pr__nfet_01v8 L=0.5 W=1\n.ends\n.GLOBAL VSS\n";
    let (d, others) = flatten(spice, "top", &|n| n.strip_prefix('X').map(str::to_string));
    let names: Vec<&str> = d.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, ["M1", "x1/M2", "x1/x2/M3"]);
    // Los pines del sub-circuito son las redes de la instancia; lo
    // interno lleva la ruta; VSS es global.
    assert_eq!(d[1].pins, ["c", "b", "x1/mid", "VSS"]);
    assert_eq!(d[2].pins, ["c", "x1/mid", "VSS", "VSS"]);
    // W=W toma el valor de la instancia (2), no el de la definición (1).
    assert_eq!(d[1].w, Some(2.0));
    assert_eq!(others, ["R9"]);
}

/// Cuánto tarda con muchos transistores (`RIKU_BENCH_N`, 2000 por
/// defecto). Una cadena: M_i de n_i a n_{i+1} con compuerta g_i (las
/// compuertas tienen nombre en los dos lados, como pines), en una grilla.
///
/// `cargo test --release -p riku --lib escala -- --ignored --nocapture`
#[test]
#[ignore = "mide tiempos"]
fn escala() {
    let n: usize = std::env::var("RIKU_BENCH_N").ok().and_then(|v| v.parse().ok()).unwrap_or(2000);
    let pins = |i: usize| [format!("n{}", i + 1), format!("g{i}"), format!("n{i}"), "VSS".to_string()];
    let place = |i: usize| ((i % 100) as f64 * 2.0, (i / 100) as f64 * 3.0);
    let sch: Vec<SchDevice> =
        (0..n).map(|i| SchDevice { name: format!("M{i}"), model: N.into(), pins: pins(i), w: Some(1.0), l: Some(0.5), m: 1.0 }).collect();
    let lay_at = |dx: f64, moved: &dyn Fn(usize) -> bool| -> Vec<LayDevice> {
        (0..n)
            .map(|i| {
                let (x, y) = place(i);
                // Los movidos sueltos, cada uno distinto (no es un movimiento rígido).
                let (x, y) = if moved(i) { (x + 0.5, y + 1.0 + i as f64 * 0.013) } else { (x + dx, y) };
                LayDevice { model: N.into(), at: (x, y), gate: [0.0; 4], cell: None, local: (x, y), w: 1.0, l: 0.5, pins: pins(i) }
            })
            .collect()
    };
    let lay = lay_at(0.0, &|_| false);
    let time = |what: &str, f: &mut dyn FnMut() -> usize| {
        let t = std::time::Instant::now();
        let r = f();
        eprintln!("[escala] n={n:<6} {what:<34} {:>9.3} s  ({r})", t.elapsed().as_secs_f64());
    };
    let mut map = MapFile::new("a.sch", "a.gds", None);
    time("sugerir desde cero", &mut || {
        map.binds = suggest(&MapFile::new("a.sch", "a.gds", None), &sch, &lay);
        map.binds.len()
    });
    time("chequear todo vinculado", &mut || check(&map, &sch, &lay).bound.len());
    let all_moved = lay_at(7.0, &|_| false);
    time("chequear todo movido (rígido)", &mut || check(&map, &sch, &all_moved).moved.map_or(0, |m| m.count));
    let some_moved = lay_at(0.0, &|i| i % 10 == 0);
    time("chequear 10% movido suelto", &mut || check(&map, &sch, &some_moved).by_connectivity.len());
}

#[test]
fn el_archivo_ida_y_vuelta() {
    let m = full();
    let text = m.to_text();
    assert!(text.contains("[[bind]]\nschematic = \"M2\""), "{text}");
    assert_eq!(MapFile::parse(&text).unwrap(), m);
}

#[test]
fn lee_la_netlist_del_esquematico() {
    let spice = "** t\n.subckt t in out\nXM1 out in VSS VSS sky130_fd_pr__nfet_01v8 L=0.5 W=18 nf=4\n+ m=2\nR1 a b 1k\n.ends\n.subckt otra a\nXM9 a a a a sky130_fd_pr__nfet_01v8 L=1 W=1\n.ends\n";
    let d = schematic_devices(spice, "t", &|n| n.strip_prefix('X').map(str::to_string));
    assert_eq!(d.len(), 1);
    assert_eq!((d[0].name.as_str(), d[0].w, d[0].l, d[0].m), ("M1", Some(18.0), Some(0.5), 2.0));
    assert_eq!(microns("0.5u"), Some(0.5));
    assert_eq!(microns("500n"), Some(0.5));
    assert_eq!(microns("1e-6"), Some(1e-6));
    assert_eq!(microns("W_N"), None);
}
