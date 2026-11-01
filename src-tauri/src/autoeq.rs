//! Buscador de AutoEq (github.com/jaakkopasanen/AutoEq, MIT): índice de correcciones por modelo de cascos,
//! guardado en caché una semana para que el buscador funcione sin conexión.

use serde::Serialize;
use std::path::Path;
use std::time::{Duration, SystemTime};

const RAW: &str = "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results";
const TARGETS: &str = "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/targets";
/// Objetivos que se ofrecen (archivos de `targets/` del repo de AutoEq)
pub const TARGET_FILES: [&str; 5] = ["Harman over-ear 2018", "Harman in-ear 2019", "Diffuse field 5128", "Diffuse field ISO 11904-1", "AutoEq in-ear"];
const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    /// Quién lo midió: oratory1990, crinacle, Rtings…
    pub source: String,
    /// `over-ear`, `in-ear` o `earbud`
    pub form: String,
    /// Ruta dentro de `results/`, tal cual viene en el índice (codificada para URL)
    pub path: String,
}

/// `- [Nombre](./fuente/tipo/Nombre) by fuente on banco`
pub fn parse_index(text: &str) -> Vec<Entry> {
    text.lines()
        .filter_map(|l| {
            let l = l.strip_prefix("- [")?;
            let (name, rest) = l.split_once("](./")?;
            // Los nombres llevan paréntesis («(250 Ohm)»): la ruta acaba en «) by »
            let (path, rest) = rest.split_once(") by ")?;
            let source = rest.split(" on ").next()?.trim().to_string();
            let kind = path.split('/').nth(1)?.replace("%20", " ");
            let form = ["in-ear", "over-ear", "earbud"].into_iter().find(|f| kind.contains(f)).unwrap_or("over-ear").to_string();
            safe_path(path).then(|| Entry { name: name.to_string(), source, form, path: path.to_string() })
        })
        .collect()
}

/// Solo rutas del índice: nada de subir de carpeta ni caracteres raros en la URL
fn safe_path(p: &str) -> bool {
    !p.is_empty() && p.len() < 300 && !p.contains("..") && !p.starts_with('/') && p.chars().all(|c| c.is_ascii_alphanumeric() || " %()&+,.-_'!/$".contains(c))
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder().user_agent("PoxiEQ").timeout(Duration::from_secs(20)).build().map_err(|e| e.to_string())
}

/// Índice desde la caché si tiene menos de una semana; si no, de internet (y si falla, la caché vieja).
pub async fn index(cache: &Path, force: bool) -> Result<Vec<Entry>, String> {
    let fresh = std::fs::metadata(cache)
        .and_then(|m| m.modified())
        .is_ok_and(|t| SystemTime::now().duration_since(t).unwrap_or_default() < MAX_AGE);
    if fresh && !force {
        if let Ok(text) = std::fs::read_to_string(cache) {
            return Ok(parse_index(&text));
        }
    }
    let download = async {
        let text = client()?.get(format!("{RAW}/INDEX.md")).send().await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?.text().await.map_err(|e| e.to_string())?;
        let list = parse_index(&text);
        if list.len() < 100 {
            return Err("índice vacío".to_string());
        }
        if let Some(dir) = cache.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(cache, &text);
        Ok(list)
    };
    match download.await {
        Ok(list) => Ok(list),
        Err(e) => std::fs::read_to_string(cache).map(|t| parse_index(&t)).map_err(|_| e),
    }
}

/// Texto `ParametricEQ.txt` de un modelo del índice
pub async fn fetch(path: &str) -> Result<String, String> {
    if !safe_path(path) {
        return Err("ruta no válida".into());
    }
    let file = path.rsplit('/').next().unwrap_or_default();
    let url = format!("{RAW}/{path}/{file}%20ParametricEQ.txt");
    let res = client()?.get(url).send().await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
    let bytes = res.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() > 64 * 1024 {
        return Err("archivo demasiado grande".into());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

// ------------------------------------------------------- objetivos y «suena como»

/// CSV de AutoEq (`frequency,raw,…`): columnas `frequency` y las pedidas, en la rejilla de `room` (1/48 oct)
fn parse_csv(text: &str, columns: &[&str]) -> Option<Vec<Vec<f64>>> {
    let mut lines = text.lines();
    let head: Vec<&str> = lines.next()?.split(',').map(str::trim).collect();
    let fi = head.iter().position(|h| *h == "frequency")?;
    let idx: Vec<usize> = columns.iter().map(|c| head.iter().position(|h| h == c)).collect::<Option<_>>()?;
    let mut f = vec![];
    let mut cols: Vec<Vec<f64>> = vec![vec![]; columns.len()];
    for l in lines {
        let v: Vec<f64> = l.split(',').map(|x| x.trim().parse().unwrap_or(f64::NAN)).collect();
        let (Some(&freq), true) = (v.get(fi), idx.iter().all(|&i| v.get(i).is_some_and(|x| x.is_finite()))) else { continue };
        if !freq.is_finite() || freq <= 0.0 || f.last().is_some_and(|&p| freq <= p) {
            continue;
        }
        f.push(freq);
        for (c, &i) in cols.iter_mut().zip(&idx) {
            c.push(v[i]);
        }
    }
    (f.len() >= 20).then(|| cols.iter().map(|c| to_grid(&f, c)).collect())
}

/// Interpolación en frecuencia logarítmica a la rejilla de `room` (fuera de rango, el extremo)
pub fn to_grid(freqs: &[f64], vals: &[f64]) -> Vec<f64> {
    (0..crate::room::GRID)
        .map(|i| {
            let f = crate::room::grid_freq(i);
            let j = freqs.partition_point(|&x| x < f);
            if j == 0 {
                vals[0]
            } else if j >= freqs.len() {
                vals[vals.len() - 1]
            } else {
                let t = (f / freqs[j - 1]).ln() / (freqs[j] / freqs[j - 1]).ln();
                vals[j - 1] + (vals[j] - vals[j - 1]) * t
            }
        })
        .collect()
}

static CSV_CACHE: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

/// Descarga un CSV (con caché en memoria mientras la app está abierta: mover un deslizador no vuelve a bajarlo)
async fn get_csv(url: String) -> Result<String, String> {
    if let Some((_, t)) = CSV_CACHE.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(u, _)| *u == url) {
        return Ok(t.clone());
    }
    let res = client()?.get(&url).send().await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
    let bytes = res.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() > 2 << 20 {
        return Err("archivo demasiado grande".into());
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let mut c = CSV_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    c.push((url, text.clone()));
    if c.len() > 20 {
        c.remove(0);
    }
    Ok(text)
}

/// Medida cruda de un modelo del índice y el objetivo con el que lo ajustó AutoEq (rejilla de `room`)
pub async fn measurement(path: &str) -> Result<(Vec<f64>, Vec<f64>), String> {
    if !safe_path(path) {
        return Err("ruta no válida".into());
    }
    let file = path.rsplit('/').next().unwrap_or_default();
    let text = get_csv(format!("{RAW}/{path}/{file}.csv")).await?;
    let mut cols = parse_csv(&text, &["raw", "target"]).ok_or("medida no válida")?;
    let target = cols.pop().unwrap_or_default();
    Ok((cols.pop().unwrap_or_default(), target))
}

/// Uno de los objetivos de `TARGET_FILES`
pub async fn target(name: &str) -> Result<Vec<f64>, String> {
    if !TARGET_FILES.contains(&name) {
        return Err("objetivo desconocido".into());
    }
    let text = get_csv(format!("{TARGETS}/{}.csv", name.replace(' ', "%20"))).await?;
    parse_csv(&text, &["raw"]).and_then(|mut c| c.pop()).ok_or_else(|| "objetivo no válido".into())
}

/// Filtros para llevar `measured` a `target` (dB en la rejilla): hasta 16 kHz, 10 campanas, subidas de hasta 6 dB
pub fn fit(measured: &[f64], target: &[f64]) -> crate::room::Fit {
    let freqs: Vec<f64> = (0..crate::room::GRID).map(crate::room::grid_freq).collect();
    let smooth = |v: &[f64]| {
        let power: Vec<f64> = v.iter().map(|d| 10f64.powf(d / 10.0)).collect();
        crate::room::smooth(&power, 1.0 / 12.0).iter().map(|p| 10.0 * p.max(1e-30).log10()).collect::<Vec<f64>>()
    };
    let params = crate::room::FitParams { smoothing: 1.0 / 12.0, slope: 0.0, high: 16000.0, max_boost: 6.0, filters: 10 };
    crate::room::fit_curves(freqs, crate::room::normalize(&smooth(measured)), crate::room::normalize(&smooth(target)), 20.0, &params)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_a_la_rejilla_y_ajuste() {
        // Un casco con +6 dB en graves (campana a 60 Hz) contra un objetivo plano: el ajuste lo corta
        let mut text = String::from("frequency,raw,target\n");
        let mut f = 20.0;
        while f <= 20000.0 {
            let bump = 6.0 / (1.0 + ((f / 60.0f64).log2() * 1.5).powi(2));
            text.push_str(&format!("{f:.2},{bump:.3},0\n"));
            f *= 1.01;
        }
        let cols = parse_csv(&text, &["raw", "target"]).unwrap();
        assert_eq!(cols[0].len(), crate::room::GRID);
        let r = fit(&cols[0], &cols[1]);
        assert!(!r.bands.is_empty());
        let at60 = r.bands.iter().map(|b| crate::eq::band_db(b, 60.0)).sum::<f64>();
        assert!(at60 < -4.0, "{:?}", r.bands);
        assert!(parse_csv("frequency,raw\n20,1\n", &["raw"]).is_none());
    }

    #[test]
    fn lee_el_indice() {
        let text = "# Index\n\n- [Beyerdynamic DT 770 Pro](./oratory1990/over-ear/Beyerdynamic%20DT%20770%20Pro) by oratory1990\n\
- [Beyerdynamic DT 770 Pro (250 Ohm)](./crinacle/GRAS%2043AG-7%20over-ear/Beyerdynamic%20DT%20770%20Pro%20(250%20Ohm)) by crinacle on GRAS 43AG-7\n\
- [1Custom SA02](./crinacle/711%20in-ear/1Custom%20SA02) by crinacle on 711\n\
- [Malo](./a/../../etc) by nadie\n";
        let l = parse_index(text);
        assert_eq!(l.len(), 3, "la ruta con .. se descarta");
        assert_eq!(l[0], Entry { name: "Beyerdynamic DT 770 Pro".into(), source: "oratory1990".into(), form: "over-ear".into(), path: "oratory1990/over-ear/Beyerdynamic%20DT%20770%20Pro".into() });
        assert_eq!(l[1].source, "crinacle");
        assert_eq!(l[1].form, "over-ear");
        assert_eq!(l[2].form, "in-ear");
        assert!(!safe_path("a/../b"));
        assert!(!safe_path("/etc"));
    }
}

#[cfg(test)]
mod real {
    /// Con internet: `cargo test -- --ignored ajuste_real --nocapture`
    #[test]
    #[ignore]
    fn ajuste_real() {
        tauri::async_runtime::block_on(async {
            let path = "oratory1990/over-ear/Beyerdynamic%20DT%20770%20Pro";
            let (m, own) = super::measurement(path).await.unwrap();
            let harman = super::target("Harman over-ear 2018").await.unwrap();
            for (n, t) in [("suyo", own), ("harman", harman)] {
                let f = super::fit(&m, &t);
                println!("{n}: {:?}", f.bands.iter().map(|b| (b.freq.round(), b.gain, b.q)).collect::<Vec<_>>());
                assert!(f.bands.len() >= 3);
            }
        });
    }
}
