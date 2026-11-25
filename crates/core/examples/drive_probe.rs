//! Prueba manual (solo lectura): ¿se puede leer la vinculación de Google y la bóveda en Drive?
//! `cargo run -p sv-core --example drive_probe`
fn main() {
    let paths = sv_core::paths::Paths::default();
    let drive = sv_core::drive::Drive::new(paths);
    let s = drive.status();
    println!("credentials.json: {} · vinculado: {} · correo guardado: {}", s.has_credentials, s.linked, s.email.is_some());
    if s.linked {
        match drive.download() {
            Ok(Some(env)) => println!("bóveda en Drive: sí (guardada {})", env.updated_at),
            Ok(None) => println!("bóveda en Drive: no hay"),
            Err(e) => println!("error: {e}"),
        }
    }
}
