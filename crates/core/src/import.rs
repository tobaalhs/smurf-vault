//! Importar cuentas pegadas como texto: una por línea, `usuario:contraseña`. También sirve `;`, tab,
//! ` / `, ` | ` o un espacio (igual que parseImport en la versión Electron).
use regex::Regex;
use std::sync::OnceLock;

fn line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\S+?)\s*(?::|;|\t|\s/\s|\s\|\s|\s)\s*(\S+)$").unwrap())
}

/// (usuario, contraseña) de cada línea reconocida.
pub fn parse_import(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|l| line_re().captures(l).map(|c| (c[1].to_string(), c[2].to_string())))
        .collect()
}

/// Líneas con algo escrito (para mostrar cuántas se ignoraron).
pub fn non_empty_lines(text: &str) -> usize {
    text.lines().filter(|l| !l.trim().is_empty()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_every_separator_and_skips_junk() {
        let text = "cuenta1:clave1\n smurf_2 otraClave \nuser3;pw3\nuser4\tpw4\nuser5 / pw5\nuser6 | pw6\n\nsolo-una-palabra\ncon:dos:dospuntos";
        let rows = parse_import(text);
        let users: Vec<&str> = rows.iter().map(|(u, _)| u.as_str()).collect();
        assert_eq!(users, ["cuenta1", "smurf_2", "user3", "user4", "user5", "user6", "con"]);
        assert_eq!(rows[1].1, "otraClave");
        assert_eq!(rows[6].1, "dos:dospuntos", "la contraseña puede tener ':'");
        assert_eq!(non_empty_lines(text), 8);
    }
}
