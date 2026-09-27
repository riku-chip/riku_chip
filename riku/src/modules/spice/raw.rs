//! Lector de archivos `.raw` de ngspice (formato de Berkeley SPICE3).
//!
//! Un `.raw` es una secuencia de *plots* (un análisis cada uno: `op`,
//! `tran`, `ac`…). Cada plot tiene una cabecera de texto y después los
//! datos, en binario (`Binary:`, `f64` little-endian) o en texto
//! (`Values:`). La primera variable es la independiente (tiempo, frecuencia
//! o la fuente barrida).
//!
//! Los análisis complejos (`ac`, `noise`…) se guardan como magnitud en dB,
//! que es lo que se mira y compara; la frecuencia queda como su parte real.

use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub struct RawFile {
    pub plots: Vec<Plot>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plot {
    pub title: String,
    /// `Plotname:` (`Transient Analysis`, `AC Analysis`…).
    pub name: String,
    /// `Command:` (`ngspice-46, Build …`).
    pub command: Option<String>,
    /// `true` si los datos eran complejos: `values` tiene la magnitud en dB
    /// y `Variable::complex` los valores originales.
    pub complex: bool,
    /// La primera es la variable independiente.
    pub vars: Vec<Variable>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Variable {
    pub name: String,
    /// Tipo según ngspice: `time`, `frequency`, `voltage`, `current`…
    pub kind: String,
    /// Lo que se muestra y compara: el valor real, o la magnitud en dB si el
    /// análisis es complejo (la variable independiente, parte real).
    pub values: Vec<f64>,
    /// Valores complejos originales `(re, im)` de un análisis complejo. Las
    /// expresiones (`v(out)/v(in)`) operan sobre ellos, no sobre los dB.
    pub complex: Option<Vec<(f64, f64)>>,
}

impl Variable {
    /// Unidad para mostrar.
    pub fn unit(&self, complex: bool) -> &'static str {
        match self.kind.as_str() {
            "time" => "s",
            "frequency" => "Hz",
            // Resultados de expresiones con unidad conocida o sin unidad.
            "db" => "dB",
            "phase" => "°",
            "expression" => "",
            _ if complex => "dB",
            "voltage" => "V",
            "current" => "A",
            _ => "",
        }
    }
}

impl Plot {
    pub fn x(&self) -> Option<&Variable> {
        self.vars.first()
    }

    /// Variables dependientes (todas menos la primera).
    pub fn signals(&self) -> &[Variable] {
        self.vars.get(1..).unwrap_or(&[])
    }

    pub fn points(&self) -> usize {
        self.x().map_or(0, |x| x.values.len())
    }

    /// El eje X se ve mejor en escala logarítmica (barridos en frecuencia).
    pub fn log_x(&self) -> bool {
        self.x().is_some_and(|x| x.kind == "frequency")
    }

    pub fn signal(&self, name: &str) -> Option<&Variable> {
        self.signals().iter().find(|v| v.name.eq_ignore_ascii_case(name))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawError(pub String);

impl fmt::Display for RawError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RawError {}

fn err<T>(msg: impl Into<String>) -> Result<T, RawError> {
    Err(RawError(msg.into()))
}

/// `true` si el contenido parece un `.raw` de SPICE (por su cabecera).
pub fn looks_like_raw(content: &[u8]) -> bool {
    let head = &content[..content.len().min(4096)];
    head.starts_with(b"Title:") && contains(head, b"\nPlotname:") && contains(head, b"\nVariables:")
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

pub fn parse(content: &[u8]) -> Result<RawFile, RawError> {
    let mut plots = Vec::new();
    let mut pos = 0;
    while pos < content.len() {
        // Saltar blancos entre plots.
        while pos < content.len() && content[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= content.len() {
            break;
        }
        // Tras el primer plot solo sigue otro si empieza con su cabecera; lo
        // demás es la cola de un archivo cortado (simulación interrumpida).
        if !plots.is_empty() && !content[pos..].starts_with(b"Title:") {
            break;
        }
        let (plot, next) = parse_plot(content, pos)?;
        plots.push(plot);
        pos = next;
    }
    if plots.is_empty() {
        return err("archivo .raw sin análisis");
    }
    Ok(RawFile { plots })
}

/// Lee una línea desde `pos`; devuelve la línea (sin `\n` ni `\r`) y el
/// inicio de la siguiente.
fn line(content: &[u8], pos: usize) -> Option<(&str, usize)> {
    if pos >= content.len() {
        return None;
    }
    let end = content[pos..].iter().position(|&b| b == b'\n').map_or(content.len(), |i| pos + i);
    let text = std::str::from_utf8(&content[pos..end]).ok()?;
    Some((text.trim_end_matches('\r'), (end + 1).min(content.len())))
}

fn parse_plot(content: &[u8], mut pos: usize) -> Result<(Plot, usize), RawError> {
    let mut title = String::new();
    let mut name = String::new();
    let mut command = None;
    let mut complex = false;
    let mut n_vars: Option<usize> = None;
    let mut n_points: Option<usize> = None;
    let mut vars: Vec<(String, String)> = Vec::new();

    loop {
        let Some((l, next)) = line(content, pos) else {
            return err("cabecera .raw incompleta (falta Binary: o Values:)");
        };
        pos = next;
        let (key, value) = match l.split_once(':') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => (l.trim(), ""),
        };
        match key {
            "Title" => title = value.to_string(),
            "Plotname" => name = value.to_string(),
            "Command" => command = Some(value.to_string()),
            "Flags" => complex = value.split_whitespace().any(|f| f.eq_ignore_ascii_case("complex")),
            "No. Variables" => n_vars = value.parse().ok(),
            "No. Points" => n_points = value.parse().ok(),
            "Variables" => {
                let n = n_vars.ok_or_else(|| RawError("Variables: antes de No. Variables:".into()))?;
                // Puede venir la primera variable en la misma línea.
                let mut pending: Vec<String> = if value.is_empty() { vec![] } else { vec![value.to_string()] };
                while vars.len() < n {
                    let text = match pending.pop() {
                        Some(t) => t,
                        None => {
                            let (l, next) = line(content, pos).ok_or_else(|| RawError("lista de variables cortada".into()))?;
                            pos = next;
                            l.to_string()
                        }
                    };
                    let mut parts = text.split_whitespace();
                    let (Some(_idx), Some(vname), Some(vkind)) = (parts.next(), parts.next(), parts.next()) else {
                        return err(format!("variable mal formada: {text:?}"));
                    };
                    vars.push((vname.to_string(), vkind.to_string()));
                }
            }
            "Binary" | "Values" => {
                let n = vars.len();
                if n == 0 {
                    return err("plot sin variables");
                }
                let expected = n_points.unwrap_or(usize::MAX);
                let (columns, next) = if key == "Binary" {
                    read_binary(content, pos, n, expected, complex)
                } else {
                    read_ascii(content, pos, n, expected, complex)?
                };
                let vars = vars
                    .into_iter()
                    .zip(columns)
                    .enumerate()
                    .map(|(c, ((name, kind), pairs))| {
                        let values = pairs.iter().map(|&(re, im)| if complex { complex_value(c, re, im) } else { re }).collect();
                        Variable { name, kind, values, complex: complex.then_some(pairs) }
                    })
                    .collect();
                return Ok((Plot { title, name, command, complex, vars }, next));
            }
            // Date, Dimensions, Option, Command… que no se usan.
            _ => {}
        }
    }
}

/// Convierte un par complejo al valor que se guarda: la variable
/// independiente (frecuencia) queda como parte real; el resto, magnitud en dB.
fn complex_value(col: usize, re: f64, im: f64) -> f64 {
    if col == 0 {
        re
    } else {
        20.0 * (re.hypot(im)).max(1e-300).log10()
    }
}

/// Datos binarios. Si el archivo se cortó (simulación interrumpida), se
/// leen los puntos completos que haya.
fn read_binary(content: &[u8], pos: usize, n: usize, expected: usize, complex: bool) -> (Vec<Vec<(f64, f64)>>, usize) {
    let width = if complex { 16 } else { 8 };
    let row = n * width;
    let available = (content.len() - pos) / row;
    let points = available.min(expected);
    let mut columns = vec![Vec::with_capacity(points); n];
    let f = |at: usize| f64::from_le_bytes(content[at..at + 8].try_into().unwrap());
    for p in 0..points {
        let base = pos + p * row;
        for (c, col) in columns.iter_mut().enumerate() {
            let at = base + c * width;
            col.push(if complex { (f(at), f(at + 8)) } else { (f(at), 0.0) });
        }
    }
    (columns, pos + points * row)
}

/// Datos en texto: por punto, `<idx>\t<v0>` y después una línea por variable.
/// Complejos como `re,im`.
fn read_ascii(content: &[u8], mut pos: usize, n: usize, expected: usize, complex: bool) -> Result<(Vec<Vec<(f64, f64)>>, usize), RawError> {
    let mut columns: Vec<Vec<(f64, f64)>> = vec![Vec::new(); n];
    let parse_val = |_c: usize, tok: &str| -> Result<(f64, f64), RawError> {
        let bad = || RawError(format!("valor no numérico: {tok:?}"));
        if complex {
            let (re, im) = tok.split_once(',').ok_or_else(bad)?;
            Ok((re.trim().parse().map_err(|_| bad())?, im.trim().parse().map_err(|_| bad())?))
        } else {
            Ok((tok.parse().map_err(|_| bad())?, 0.0))
        }
    };
    let mut points = 0;
    'points: while points < expected {
        // Línea con índice y primer valor.
        let mut row = Vec::with_capacity(n);
        while row.len() < n {
            let Some((l, next)) = line(content, pos) else { break 'points };
            let t = l.trim();
            if t.is_empty() {
                if row.is_empty() {
                    pos = next;
                    continue;
                }
                break 'points;
            }
            // Empieza otro plot.
            if row.is_empty() && t.starts_with("Title:") {
                break 'points;
            }
            pos = next;
            let mut toks = t.split_whitespace();
            if row.is_empty() {
                toks.next(); // índice del punto
            }
            for tok in toks {
                // Más valores que variables: el archivo no es lo que dice
                // la cabecera (antes, un índice fuera de rango y pánico).
                if row.len() == n {
                    return err(format!("punto {points}: más de {n} valores (No. Variables: {n})"));
                }
                row.push(parse_val(row.len(), tok)?);
            }
        }
        for (c, v) in row.into_iter().enumerate() {
            columns[c].push(v);
        }
        points += 1;
    }
    Ok((columns, pos))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// `.raw` binario de un plot real con `vars` y columnas dadas.
    pub(crate) fn binary_raw(plotname: &str, vars: &[(&str, &str)], columns: &[Vec<f64>]) -> Vec<u8> {
        let points = columns[0].len();
        let mut out = format!(
            "Title: prueba\nDate: hoy\nCommand: ngspice-46, Build x\nPlotname: {plotname}\nFlags: real\nNo. Variables: {}\nNo. Points: {points}\nVariables:\n",
            vars.len()
        );
        for (i, (n, k)) in vars.iter().enumerate() {
            out += &format!("\t{i}\t{n}\t{k}\n");
        }
        out += "Binary:\n";
        let mut bytes = out.into_bytes();
        for p in 0..points {
            for col in columns {
                bytes.extend_from_slice(&col[p].to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn reads_binary_real_plot() {
        let raw = binary_raw("Transient Analysis", &[("time", "time"), ("v(out)", "voltage")], &[vec![0.0, 1e-9, 2e-9], vec![0.0, 0.5, 1.0]]);
        assert!(looks_like_raw(&raw));
        let f = parse(&raw).unwrap();
        assert_eq!(f.plots.len(), 1);
        let p = &f.plots[0];
        assert_eq!(p.name, "Transient Analysis");
        assert_eq!(p.command.as_deref(), Some("ngspice-46, Build x"));
        assert_eq!(p.points(), 3);
        assert_eq!(p.signal("V(OUT)").unwrap().values, vec![0.0, 0.5, 1.0]);
        assert_eq!(p.signal("v(out)").unwrap().unit(false), "V");
    }

    #[test]
    fn truncated_binary_keeps_complete_points() {
        let mut raw = binary_raw("Transient Analysis", &[("time", "time"), ("v(a)", "voltage")], &[vec![0.0, 1.0, 2.0], vec![5.0, 6.0, 7.0]]);
        raw.truncate(raw.len() - 4);
        let p = &parse(&raw).unwrap().plots[0];
        assert_eq!(p.points(), 2);
        assert_eq!(p.signals()[0].values, vec![5.0, 6.0]);
    }

    #[test]
    fn reads_ascii_with_several_plots_and_complex() {
        let text = "Title: t\nDate: d\nPlotname: Operating Point\nFlags: real\nNo. Variables: 2\nNo. Points: 1\nVariables:\n\t0\tv(a)\tvoltage\n\t1\ti(v1)\tcurrent\nValues:\n 0\t1.8\n\t-2.5e-03\n\n\
Title: t\nDate: d\nPlotname: AC Analysis\nFlags: complex\nNo. Variables: 2\nNo. Points: 2\nVariables:\n\t0\tfrequency\tfrequency grid=3\n\t1\tv(out)\tvoltage\nValues:\n 0\t1.000e+00,0.0\n\t1.0,0.0\n 1\t1.000e+03,0.0\n\t0.0,0.1\n";
        let f = parse(text.as_bytes()).unwrap();
        assert_eq!(f.plots.len(), 2);
        assert_eq!(f.plots[0].vars[1].values, vec![-2.5e-3]);
        let ac = &f.plots[1];
        assert!(ac.complex && ac.log_x());
        assert_eq!(ac.x().unwrap().values, vec![1.0, 1000.0]);
        let db = &ac.signal("v(out)").unwrap().values;
        assert!((db[0] - 0.0).abs() < 1e-12 && (db[1] + 20.0).abs() < 1e-9);
        assert_eq!(ac.signal("v(out)").unwrap().unit(true), "dB");
        // Los complejos originales se conservan para las expresiones.
        assert_eq!(ac.signal("v(out)").unwrap().complex.as_deref(), Some(&[(1.0, 0.0), (0.0, 0.1)][..]));
    }

    #[test]
    fn ascii_with_more_values_than_variables_is_an_error_not_a_panic() {
        let text = "Title: t
Date: d
Plotname: Transient Analysis
Flags: real
No. Variables: 2
No. Points: 2
Variables:
	0	time	time
	1	v(a)	voltage
Values:
 0	0.0	1.0	9.9
 1	1.0	2.0
";
        let e = parse(text.as_bytes()).unwrap_err();
        assert!(e.to_string().contains("más de 2 valores"), "{e}");
        // Igual si el valor de más viene en la línea siguiente.
        let text = text.replace("	1.0	9.9
", "	1.0
	9.9	8.8
");
        assert!(parse(text.as_bytes()).is_err());
    }

    #[test]
    fn rejects_non_raw() {
        assert!(!looks_like_raw(b"v {xschem version=3.4.5}"));
        assert!(parse(b"Title: x\nPlotname: y\n").is_err());
    }
}
