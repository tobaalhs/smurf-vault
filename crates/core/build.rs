//! Mete el credentials.json de Google (raíz del repo, no va en git) dentro del programa, como lo
//! hacía el instalador de Electron. Sin el archivo, el programa pide cargarlo en Ajustes.
fn main() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../credentials.json");
    println!("cargo:rerun-if-changed={}", src.display());
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("credentials.json");
    let text = std::fs::read_to_string(&src).unwrap_or_default();
    std::fs::write(out, text).unwrap();
}
