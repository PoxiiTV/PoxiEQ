//! Buscador de AutoEq (github.com/jaakkopasanen/AutoEq, MIT): índice de correcciones por modelo de cascos,
//! guardado en caché una semana para que el buscador funcione sin conexión.

use serde::Serialize;
use std::path::Path;
use std::time::{Duration, SystemTime};

const RAW: &str = "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results";
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

#[cfg(test)]
mod tests {
    use super::*;

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
