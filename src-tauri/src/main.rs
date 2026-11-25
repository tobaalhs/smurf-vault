// Sin consola de Windows en la versión compilada (release).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    smurf_vault_lib::run()
}
