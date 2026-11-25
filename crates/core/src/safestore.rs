//! Cifrado de secretos locales con el mismo formato que `safeStorage` de Electron en Windows, para
//! que el token de Google sirva igual en la versión Electron y en esta:
//! - La clave (32 bytes) va en "Local State" → os_crypt.encrypted_key, como "DPAPI" + blob de
//!   CryptProtectData en base64: solo tu usuario de Windows puede leerla.
//! - Cada secreto es "v10" + nonce (12 bytes) + AES-256-GCM(texto) con la etiqueta al final.
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use rand::RngCore;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const PREFIX: &[u8] = b"v10";
const DPAPI_PREFIX: &[u8] = b"DPAPI";

#[cfg(windows)]
mod dpapi {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB};

    fn run(data: &[u8], protect: bool) -> Option<Vec<u8>> {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        // SAFETY: entradas válidas mientras dura la llamada; la salida la reserva Windows y se
        // libera con LocalFree después de copiarla.
        let ok = unsafe {
            if protect {
                CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), 0, &mut output)
            } else {
                CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), 0, &mut output)
            }
        };
        if ok == 0 || output.pbData.is_null() {
            return None;
        }
        // SAFETY: Windows devolvió cbData bytes en pbData.
        let out = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
        unsafe { LocalFree(output.pbData as _) };
        Some(out)
    }

    pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
        run(data, true)
    }

    pub fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
        run(data, false)
    }
}

#[cfg(not(windows))]
mod dpapi {
    pub fn protect(_: &[u8]) -> Option<Vec<u8>> {
        None
    }
    pub fn unprotect(_: &[u8]) -> Option<Vec<u8>> {
        None
    }
}

pub struct SafeStore {
    local_state: PathBuf,
}

impl SafeStore {
    pub fn new(data_dir: &Path) -> Self {
        Self { local_state: data_dir.join("Local State") }
    }

    fn read_state(&self) -> Value {
        std::fs::read_to_string(&self.local_state).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| json!({}))
    }

    /// La clave de "Local State"; si no existe, se crea (y se guarda sin tocar el resto del archivo).
    fn key(&self, create: bool) -> Option<[u8; 32]> {
        let mut state = self.read_state();
        if let Some(enc) = state["os_crypt"]["encrypted_key"].as_str() {
            let raw = base64::engine::general_purpose::STANDARD.decode(enc).ok()?;
            let key = dpapi::unprotect(raw.strip_prefix(DPAPI_PREFIX)?)?;
            return key.try_into().ok();
        }
        if !create {
            return None;
        }
        let mut key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        let blob = [DPAPI_PREFIX, &dpapi::protect(&key)?].concat();
        if !state.is_object() {
            state = json!({});
        }
        state["os_crypt"]["encrypted_key"] = json!(base64::engine::general_purpose::STANDARD.encode(blob));
        crate::paths::write_atomic(&self.local_state, state.to_string().as_bytes()).ok()?;
        Some(key)
    }

    pub fn encrypt(&self, text: &str) -> Result<Vec<u8>, String> {
        let key = self.key(true).ok_or("No pude usar el cifrado de Windows")?;
        let mut nonce = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce);
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| e.to_string())?;
        let ct = cipher.encrypt(Nonce::from_slice(&nonce), text.as_bytes()).map_err(|e| e.to_string())?;
        Ok([PREFIX, &nonce, &ct].concat())
    }

    pub fn decrypt(&self, data: &[u8]) -> Option<String> {
        let rest = data.strip_prefix(PREFIX)?;
        if rest.len() < 12 + 16 {
            return None;
        }
        let key = self.key(false)?;
        let cipher = Aes256Gcm::new_from_slice(&key).ok()?;
        let plain = cipher.decrypt(Nonce::from_slice(&rest[..12]), &rest[12..]).ok()?;
        String::from_utf8(plain).ok()
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_reuses_the_key() {
        let dir = std::env::temp_dir().join(format!("sv-safe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = SafeStore::new(&dir);
        let blob = store.encrypt(r#"{"refresh_token":"x"}"#).unwrap();
        assert!(blob.starts_with(b"v10"));
        assert_eq!(SafeStore::new(&dir).decrypt(&blob).as_deref(), Some(r#"{"refresh_token":"x"}"#));
        assert!(store.decrypt(b"v10short").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
