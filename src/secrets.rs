//! User-bound Windows DPAPI storage; plaintext never crosses the read API.
use anyhow::{bail, Result};

#[cfg(windows)]
pub fn protect(bytes: &[u8], decrypt: bool) -> Result<Vec<u8>> {
    use std::ptr::{null, null_mut};
    #[repr(C)]
    struct Blob {
        len: u32,
        data: *mut u8,
    }
    #[link(name = "crypt32")]
    extern "system" {
        fn CryptProtectData(
            input: *const Blob,
            description: *const u16,
            entropy: *const Blob,
            reserved: *mut std::ffi::c_void,
            prompt: *mut std::ffi::c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
        fn CryptUnprotectData(
            input: *const Blob,
            description: *mut *mut u16,
            entropy: *const Blob,
            reserved: *mut std::ffi::c_void,
            prompt: *mut std::ffi::c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(memory: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    }
    if bytes.is_empty() || bytes.len() > 16384 {
        bail!("Credencial inválida");
    }
    let input = Blob {
        len: bytes.len() as u32,
        data: bytes.as_ptr() as *mut u8,
    };
    let context = b"NetherGate DDNS v1";
    let entropy = Blob {
        len: context.len() as u32,
        data: context.as_ptr() as *mut u8,
    };
    let mut output = Blob {
        len: 0,
        data: null_mut(),
    };
    let ok = unsafe {
        if decrypt {
            CryptUnprotectData(
                &input,
                null_mut(),
                &entropy,
                null_mut(),
                null_mut(),
                1,
                &mut output,
            )
        } else {
            CryptProtectData(
                &input,
                null(),
                &entropy,
                null_mut(),
                null_mut(),
                1,
                &mut output,
            )
        }
    };
    if ok == 0 {
        bail!("O Windows não conseguiu proteger ou ler a credencial. Volta a introduzir o token nesta conta do Windows");
    }
    let value = unsafe { std::slice::from_raw_parts(output.data, output.len as usize).to_vec() };
    unsafe {
        LocalFree(output.data.cast());
    }
    Ok(value)
}

#[cfg(not(windows))]
pub fn protect(_bytes: &[u8], _decrypt: bool) -> Result<Vec<u8>> {
    bail!("As credenciais DDNS protegidas estão disponíveis apenas no Windows")
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn credentials_roundtrip_and_reject_corruption() {
        let token = b"example-private-credential";
        let encrypted = protect(token, false).unwrap();
        assert!(!encrypted.windows(token.len()).any(|w| w == token));
        assert_eq!(protect(&encrypted, true).unwrap(), token);
        let mut broken = encrypted;
        broken[0] ^= 1;
        assert!(protect(&broken, true).is_err());
    }
}
