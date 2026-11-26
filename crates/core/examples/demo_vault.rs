//! Crea una bóveda de demostración con cuentas ficticias (rangos, partidas, LP, un baneo, marcos)
//! para probar la interfaz sin tocar la bóveda real:
//!
//!   cargo run -p sv-core --example demo_vault -- <carpeta>
//!   SV_DATA_DIR=<carpeta> SV_DEV_UNLOCK=demo-password cargo run -p smurf-vault
use serde_json::json;
use sv_core::matches::Match;
use sv_core::model::{Account, Rank, Ranks};
use sv_core::paths::Paths;
use sv_core::store::{now_iso, Opened, Store};

const PASSWORD: &str = "demo-password";

fn rank(tier: &str, division: &str, lp: i64, wins: i64, losses: i64) -> Option<Rank> {
    Some(Rank { tier: tier.into(), division: division.into(), lp, wins, losses, ..Default::default() })
}

fn days_ago(d: i64) -> String {
    (chrono::Utc::now() - chrono::Duration::hours(d * 24)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn lol(n: i64, champ: i64, win: bool, k: i64, d: i64, a: i64) -> Match {
    let at = chrono::Utc::now() - chrono::Duration::hours(n * 9 + 2);
    sv_core::matches::from_lol_game(&json!({
        "gameId": 1000 + n, "gameCreation": at.timestamp_millis() - 1800_000, "gameDuration": 1800, "queueId": if n % 3 == 0 { 440 } else { 420 },
        "participants": [{ "championId": champ, "stats": { "win": win, "kills": k, "deaths": d, "assists": a,
            "totalMinionsKilled": 160 + n * 7, "neutralMinionsKilled": 12, "champLevel": 15 + n % 3,
            "item0": 3089, "item1": 3020, "item2": 3157, "item3": 3135, "item4": 3165, "item5": 0, "item6": 3363 } }]
    }))
    .unwrap()
}

fn tft(n: i64, placement: i64) -> Match {
    let at = chrono::Utc::now() - chrono::Duration::hours(n * 11 + 5);
    sv_core::matches::from_tft_game(
        &json!({ "metadata": { "match_id": format!("LA2_{n}") }, "json": { "game_datetime": at.timestamp_millis() - 1500_000, "game_length": 1500.0, "queue_id": 1100,
            "participants": [{ "puuid": "me", "placement": placement, "level": 8 }] } }),
        "me",
    )
    .unwrap()
}

fn main() {
    let dir = std::env::args().nth(1).expect("uso: demo_vault <carpeta>");
    let _ = std::fs::remove_dir_all(&dir);
    let mut store = Store::new(Paths::at(&dir));
    store.finish_create(Opened::new_vault(PASSWORD).unwrap()).unwrap();

    let base = |id: &str, game: &str, tag: &str, user: &str| Account {
        id: id.into(),
        created_at: Some(now_iso()),
        updated_at: Some(now_iso()),
        username: user.into(),
        password: "demo".into(),
        server: "LAS".into(),
        game_name: game.into(),
        tag_line: tag.into(),
        puuid: Some(format!("puuid-{id}")),
        last_synced_at: Some(days_ago(1)),
        ..Default::default()
    };
    let accounts = vec![
        Account {
            label: "Main".into(),
            ranks: Ranks { solo: rank("DIAMOND", "III", 47, 112, 98), flex: rank("EMERALD", "I", 20, 30, 22), ..Default::default() },
            level: Some(412),
            icon_id: Some(29),
            background_skin_id: Some(103000),
            last_played_at: Some(days_ago(0)),
            favorite: Some(true),
            notes: "Cuenta principal. Correo: demo@example.com".into(),
            mastery: Some(json!({ "score": 486, "top": [
                { "champ": 103, "level": 42, "points": 512340 }, { "champ": 157, "level": 18, "points": 214870 },
                { "champ": 64, "level": 11, "points": 120400 }, { "champ": 222, "level": 7, "points": 64210 },
                { "champ": 84, "level": 5, "points": 30120 } ] })),
            ..base("a1", "Demo Mid", "LAS", "demo_mid")
        },
        Account {
            ranks: Ranks { solo: rank("GOLD", "II", 64, 40, 37), tft: rank("PLATINUM", "IV", 10, 12, 9), ..Default::default() },
            level: Some(87),
            icon_id: Some(7),
            background_skin_id: Some(222000),
            last_played_at: Some(days_ago(3)),
            crest: Some(json!({ "type": "level", "theme": 5 })),
            ..base("a2", "Smurf Jinx", "ADC", "smurf_jinx")
        },
        Account {
            ranks: Ranks::default(),
            level: Some(31),
            icon_id: Some(1),
            ..base("a3", "Sin Rango", "LAS", "nuevo_lvl30")
        },
        Account {
            label: "Castigada".into(),
            ranks: Ranks { solo: rank("SILVER", "I", 90, 20, 25), ..Default::default() },
            level: Some(55),
            icon_id: Some(4),
            background_skin_id: Some(157000),
            ban: Some(json!({ "until": (chrono::Utc::now() + chrono::Duration::hours(53)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true) })),
            ..base("a4", "Yasuo Main", "0/10", "yasuo_main")
        },
        Account {
            ranks: Ranks { solo: rank("MASTER", "", 132, 210, 180), ..Default::default() },
            level: Some(523),
            icon_id: Some(3478),
            background_skin_id: Some(64000),
            last_played_at: Some(days_ago(12)),
            ..base("a5", "Lee Sin Kick", "INSEC", "leesin_insec")
        },
    ];

    let s = store.session_mut().unwrap();
    s.data.accounts = accounts;
    let a1: Vec<Match> = (0..10).map(|n| lol(n, [103, 157, 103, 64, 103][n as usize % 5], n % 3 != 1, 7 + n % 4, 2 + n % 3, 9)).collect();
    s.history.add("a1", &a1);
    let a2: Vec<Match> = (0..6).map(|n| lol(n, 222, n % 2 == 0, 9, 4, 6)).chain((0..8).map(|n| tft(n, [1, 4, 6, 2, 8, 3, 5, 4][n as usize]))).collect();
    s.history.add("a2", &a2);
    s.history.add("a5", &(0..5).map(|n| lol(n, 64, true, 12, 3, 10)).collect::<Vec<_>>());
    let points = [("DIAMOND", "IV", 70), ("DIAMOND", "IV", 95), ("DIAMOND", "III", 15), ("DIAMOND", "IV", 88), ("DIAMOND", "III", 22), ("DIAMOND", "III", 47)];
    for (i, (t, d, lp)) in points.iter().enumerate() {
        let r = Ranks { solo: rank(t, d, *lp, 0, 0), ..Default::default() };
        s.lp.record("a1", &r, &days_ago(10 - i as i64));
    }
    s.lp.record("a2", &Ranks { solo: rank("GOLD", "II", 40, 0, 0), ..Default::default() }, &days_ago(5));
    s.lp.record("a2", &Ranks { solo: rank("GOLD", "II", 64, 0, 0), ..Default::default() }, &days_ago(3));
    store.persist().unwrap();
    store.save_history().unwrap();
    store.save_lp().unwrap();
    store.remember_lock_splashes();
    println!("bóveda de demostración en {dir} (contraseña: {PASSWORD})");
}
