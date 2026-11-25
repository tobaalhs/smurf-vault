//! Cifrado de la bóveda: scrypt (contraseña maestra -> clave) + AES-256-GCM.
//! Mismo formato que la versión Electron (src/vault.js): el "sobre" (envelope) es JSON con todo lo
//! necesario para descifrar menos la contraseña. El tag de GCM va aparte del texto cifrado.
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const FORMAT: &str = "smurf-vault";
const SCRYPT_N: u64 = 1 << 17;
const SCRYPT_R: u32 = 8;
const SCRYPT_P: u32 = 1;
const TAG_LEN: usize = 16;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Kdf {
    pub name: String,
    #[serde(rename = "N")]
    pub n: u64,
    pub r: u32,
    pub p: u32,
    pub salt: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    pub format: String,
    pub v: u32,
    pub kdf: Kdf,
    pub iv: String,
    pub tag: String,
    pub data: String,
    pub updated_at: String,
}

pub type Key = [u8; 32];

fn derive(password: &str, salt: &[u8], n: u64, r: u32, p: u32) -> Result<Key, String> {
    if !n.is_power_of_two() || n < 2 {
        return Err("Archivo de bóveda inválido".into());
    }
    let params = scrypt::Params::new(n.trailing_zeros() as u8, r, p, 32)
        .map_err(|_| "Archivo de bóveda inválido".to_string())?;
    let mut key = [0u8; 32];
    scrypt::scrypt(password.as_bytes(), salt, &params, &mut key)
        .map_err(|_| "No se pudo derivar la clave".to_string())?;
    Ok(key)
}

/// Clave nueva para una contraseña: { key, salt } con salt aleatorio de 16 bytes.
pub fn create_key(password: &str) -> Result<(Key, Vec<u8>), String> {
    let mut salt = vec![0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    let key = derive(password, &salt, SCRYPT_N, SCRYPT_R, SCRYPT_P)?;
    Ok((key, salt))
}

pub fn encrypt_with_key(key: &Key, salt: &[u8], data: &Value) -> Result<Envelope, String> {
    let mut iv = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut iv);
    let plain = serde_json::to_vec(data).map_err(|e| e.to_string())?;
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| e.to_string())?;
    // aes-gcm devuelve texto cifrado + tag; el formato los guarda por separado.
    let mut out = cipher
        .encrypt(Nonce::from_slice(&iv), plain.as_ref())
        .map_err(|_| "No se pudo cifrar".to_string())?;
    let tag = out.split_off(out.len() - TAG_LEN);
    Ok(Envelope {
        format: FORMAT.into(),
        v: 1,
        kdf: Kdf { name: "scrypt".into(), n: SCRYPT_N, r: SCRYPT_R, p: SCRYPT_P, salt: B64.encode(salt) },
        iv: B64.encode(iv),
        tag: B64.encode(tag),
        data: B64.encode(out),
        updated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    })
}

fn open(key: &Key, env: &Envelope) -> Option<Value> {
    let iv = B64.decode(&env.iv).ok()?;
    if iv.len() != 12 {
        return None;
    }
    let mut sealed = B64.decode(&env.data).ok()?;
    sealed.extend(B64.decode(&env.tag).ok()?);
    let cipher = Aes256Gcm::new_from_slice(key).ok()?;
    let plain = cipher.decrypt(Nonce::from_slice(&iv), sealed.as_ref()).ok()?;
    serde_json::from_slice(&plain).ok()
}

/// Descifra un sobre con la contraseña maestra. Devuelve (clave, salt, datos).
pub fn decrypt_envelope(env: &Envelope, password: &str) -> Result<(Key, Vec<u8>, Value), String> {
    if env.format != FORMAT {
        return Err("Archivo de bóveda inválido".into());
    }
    let salt = B64.decode(&env.kdf.salt).map_err(|_| "Archivo de bóveda inválido".to_string())?;
    let key = derive(password, &salt, env.kdf.n, env.kdf.r, env.kdf.p)?;
    let data = open(&key, env).ok_or_else(|| "Contraseña maestra incorrecta".to_string())?;
    Ok((key, salt, data))
}

/// Descifra un sobre con una clave ya derivada (la de la bóveda abierta).
pub fn decrypt_with_key(key: &Key, env: &Envelope) -> Result<Value, String> {
    if env.format != FORMAT {
        return Err("Archivo cifrado inválido".into());
    }
    open(key, env).ok_or_else(|| "Archivo cifrado inválido".to_string())
}

/// true si `password` es la contraseña de la clave `key` (derivada con `salt`).
#[allow(dead_code)] // se usa al portar "cambiar contraseña"
pub fn check_password(password: &str, salt: &[u8], key: &Key) -> bool {
    match derive(password, salt, SCRYPT_N, SCRYPT_R, SCRYPT_P) {
        // Comparación en tiempo constante.
        Ok(candidate) => candidate.iter().zip(key).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0,
        Err(_) => false,
    }
}

pub fn empty_vault() -> Value {
    json!({ "accounts": [], "settings": { "riotApiKey": "" } })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_PASSWORD: &str = "fixture-password";

    fn js_fixture() -> Envelope {
        let raw = include_str!("../tests/fixtures/vault-js.json");
        serde_json::from_str(raw).expect("fixture válido")
    }

    /// Un sobre creado por la versión Electron (src/vault.js) se abre igual en Rust.
    #[test]
    fn opens_a_vault_written_by_the_electron_version() {
        let (key, salt, data) = decrypt_envelope(&js_fixture(), FIXTURE_PASSWORD).unwrap();
        assert_eq!(data["accounts"][0]["username"], "usuario");
        assert_eq!(data["accounts"][0]["password"], "pässwörd ✓");
        assert_eq!(salt.len(), 16);
        // Lo que Rust vuelve a cifrar con esa clave se abre con la misma clave.
        let again = encrypt_with_key(&key, &salt, &data).unwrap();
        assert_eq!(decrypt_with_key(&key, &again).unwrap(), data);
    }

    #[test]
    fn a_wrong_password_is_rejected() {
        assert_eq!(
            decrypt_envelope(&js_fixture(), "otra-clave").unwrap_err(),
            "Contraseña maestra incorrecta"
        );
    }

    #[test]
    fn new_keys_roundtrip_and_check_the_password() {
        let (key, salt) = create_key("nueva-clave").unwrap();
        let env = encrypt_with_key(&key, &salt, &empty_vault()).unwrap();
        let (_, _, data) = decrypt_envelope(&env, "nueva-clave").unwrap();
        assert_eq!(data, empty_vault());
        assert!(check_password("nueva-clave", &salt, &key));
        assert!(!check_password("otra", &salt, &key));
    }

    /// Escribe un sobre de Rust para comprobar que la versión Electron lo abre (ver test/vault-compat).
    #[test]
    fn writes_an_envelope_for_the_electron_check() {
        let Some(out) = std::env::var_os("SV_COMPAT_OUT") else { return };
        let (key, salt) = create_key(FIXTURE_PASSWORD).unwrap();
        let data = json!({ "accounts": [{ "username": "desde-rust", "password": "ñandú ✓" }], "settings": { "riotApiKey": "" } });
        let env = encrypt_with_key(&key, &salt, &data).unwrap();
        std::fs::write(out, serde_json::to_string(&env).unwrap()).unwrap();
    }
}
