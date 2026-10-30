//! Windows' own crypto, so the cloud area needs no crate of its own: SHA-256 (CNG) for update files, and DPAPI to
//! keep the GitHub token readable only by this Windows user on this PC.

use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    BCryptHash, CryptProtectData, CryptUnprotectData, BCRYPT_SHA256_ALG_HANDLE, CRYPTPROTECT_UI_FORBIDDEN,
    CRYPT_INTEGER_BLOB,
};

/// SHA-256 as lowercase hex.
pub fn sha256_hex(data: &[u8]) -> Result<String, String> {
    let mut out = [0u8; 32];
    let status = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut out) };
    if status.is_err() {
        return Err(format!("SHA-256: NTSTATUS {:#x}", status.0));
    }
    Ok(out.iter().map(|b| format!("{b:02x}")).collect())
}

/// Encrypts for the current Windows user (DPAPI).
pub fn protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    dpapi(plain, true)
}

pub fn unprotect(sealed: &[u8]) -> Result<Vec<u8>, String> {
    dpapi(sealed, false)
}

fn dpapi(data: &[u8], seal: bool) -> Result<Vec<u8>, String> {
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        let done = if seal {
            CryptProtectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        } else {
            CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        };
        done.map_err(|e| format!("DPAPI: {e}"))?;
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData as _)));
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_of_abc() {
        assert_eq!(sha256_hex(b"abc").unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn dpapi_round_trip() {
        let sealed = protect(b"gho_example").unwrap();
        assert_ne!(sealed, b"gho_example");
        assert_eq!(unprotect(&sealed).unwrap(), b"gho_example");
    }
}
