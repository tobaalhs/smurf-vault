//! Prueba manual: lee la cuenta abierta en el cliente de LoL y muestra un resumen.
//! `cargo run -p sv-core --example lcu_probe`
fn main() {
    if std::env::args().any(|a| a == "gamedata") {
        let paths = sv_core::paths::Paths::at(std::env::temp_dir().join("sv-gamedata-probe"));
        std::fs::create_dir_all(&paths.dir).unwrap();
        let g = sv_core::gamedata::GameData::fetch_fresh(&paths).unwrap();
        println!("campeones {} · minileyendas {} · hechizos {} · ddragon {} · Ahri={}", g.champions.len(), g.companions.len(),
            g.spells.as_array().map_or(0, |s| s.len()), g.ddragon, g.champion(103));
        println!("stale: {}", g.is_stale());
        return;
    }
    let lcu = sv_core::lcu::lcu();
    println!("clientes de juego con lockfile: {}", lcu.game_clients().len());
    match lcu.current_account(true) {
        None => println!("No hay cliente de LoL abierto con sesión."),
        Some(s) => {
            println!("{} · {} · nivel {:?} · ícono {:?}", s.riot_id(), s.server, s.level, s.icon_id);
            println!("solo {:?}", s.ranks.solo.map(|r| r.map(|r| format!("{} {} {} LP", r.tier, r.division, r.lp))));
            println!("partidas {:?} · última {:?}", s.matches.map(|m| m.len()), s.last_played_at);
            println!("maestría {} · marco {:?} · baneo {:?} · fondo {:?}", s.mastery.is_some(), s.crest, s.ban, s.background_skin_id);
            println!("fase {:?}", lcu.gameflow_phase());
        }
    }
}
