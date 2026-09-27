//! Windows DPAPI 加密存储 API Key（绑当前用户账户）。失败降级，不 panic。
#![cfg(windows)]

use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
};

fn to_vec_and_free(blob: &CRYPT_INTEGER_BLOB) -> Vec<u8> {
    unsafe {
        let v = std::slice::from_raw_parts(blob.pbData, blob.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(blob.pbData as *mut core::ffi::c_void));
        v
    }
}

pub fn encrypt(plain: &[u8]) -> Option<Vec<u8>> {
    unsafe {
        let in_blob = CRYPT_INTEGER_BLOB {
            cbData: plain.len() as u32,
            pbData: plain.as_ptr() as *mut u8,
        };
        let mut out = CRYPT_INTEGER_BLOB::default();
        CryptProtectData(&in_blob, PCWSTR::null(), None, None, None, 0, &mut out).ok()?;
        Some(to_vec_and_free(&out))
    }
}

pub fn decrypt(cipher: &[u8]) -> Option<Vec<u8>> {
    unsafe {
        let in_blob = CRYPT_INTEGER_BLOB {
            cbData: cipher.len() as u32,
            pbData: cipher.as_ptr() as *mut u8,
        };
        let mut out = CRYPT_INTEGER_BLOB::default();
        CryptUnprotectData(&in_blob, None, None, None, None, 0, &mut out).ok()?;
        Some(to_vec_and_free(&out))
    }
}

/// 加密保存 API key 到文件。
pub fn save_key(path: &Path, key: &str) -> Result<(), String> {
    let enc = encrypt(key.as_bytes()).ok_or("DPAPI 加密失败")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, enc).map_err(|e| e.to_string())
}

/// 读取并解密 API key。
pub fn load_key(path: &Path) -> Option<String> {
    let enc = std::fs::read(path).ok()?;
    let dec = decrypt(&enc)?;
    String::from_utf8(dec).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpapi_roundtrip() {
        let secret = "sk-test-12345-中文";
        let enc = encrypt(secret.as_bytes()).expect("encrypt");
        assert_ne!(enc, secret.as_bytes());
        let dec = decrypt(&enc).expect("decrypt");
        assert_eq!(String::from_utf8(dec).unwrap(), secret);
    }

    #[test]
    fn save_load_roundtrip() {
        let p = std::env::temp_dir().join("at_key_test.bin");
        let _ = std::fs::remove_file(&p);
        save_key(&p, "my-secret-key").unwrap();
        assert_eq!(load_key(&p).as_deref(), Some("my-secret-key"));
        std::fs::remove_file(&p).unwrap();
    }
}
