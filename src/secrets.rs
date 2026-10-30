//! Guarda credenciais no Gerenciador de Credenciais do Windows (protegido pelo DPAPI, por usuário).

/// Tokens OAuth (JSON com access/refresh/expires_at).
pub const OAUTH: &str = "ControleDoAr/smartthings-oauth";
/// Personal Access Token, quando o usuário escolhe esse modo.
pub const PAT: &str = "ControleDoAr/smartthings-pat";

#[cfg(windows)]
mod imp {
    use std::ptr::null_mut;
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    pub fn save(target: &str, secret: &str) -> Result<(), String> {
        let mut target = wide(target);
        let mut user = wide("ControleDoAr");
        let blob = secret.as_bytes();
        let cred = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            UserName: user.as_mut_ptr(),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_ptr() as *mut u8,
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            ..Default::default()
        };
        // SAFETY: todos os ponteiros apontam para buffers vivos até o fim da chamada.
        if unsafe { CredWriteW(&cred, 0) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }

    pub fn load(target: &str) -> Option<String> {
        let target = wide(target);
        let mut p: *mut CREDENTIALW = null_mut();
        // SAFETY: em caso de sucesso, `p` aponta para memória do sistema liberada com CredFree.
        unsafe {
            if CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut p) == 0 || p.is_null() {
                return None;
            }
            let c = &*p;
            let bytes = std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec();
            CredFree(p as *const _);
            String::from_utf8(bytes).ok()
        }
    }

    pub fn delete(target: &str) {
        let target = wide(target);
        // SAFETY: string terminada em zero e viva durante a chamada.
        unsafe {
            CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0);
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn save(_: &str, _: &str) -> Result<(), String> {
        Err("só suportado no Windows".into())
    }
    pub fn load(_: &str) -> Option<String> {
        None
    }
    pub fn delete(_: &str) {}
}

pub use imp::{delete, load, save};
