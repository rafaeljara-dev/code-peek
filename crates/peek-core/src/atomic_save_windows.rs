//! Native, handle-bound replacement. No path reopen after writing confidential data.
use anyhow::{bail, Context, Result};
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::Cryptography::{
    BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
};
use windows_sys::Win32::Security::{
    GetFileSecurityW, SetSecurityDescriptorControl, DACL_SECURITY_INFORMATION, SECURITY_ATTRIBUTES,
    SE_DACL_PROTECTED,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FileRenameInfo, SetFileInformationByHandle, CREATE_NEW, DELETE,
    FILE_ATTRIBUTE_NORMAL, FILE_RENAME_INFO,
};

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn security_descriptor(target: &Path) -> Result<Option<Vec<u32>>> {
    let metadata = match fs::metadata(target) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if !metadata.is_file() || metadata.permissions().readonly() {
        bail!("El destino no es un archivo regular con escritura habilitada");
    }
    let target = wide(target);
    let mut needed = 0;
    // SAFETY: valid NUL-terminated filename and output length; first call sizes buffer.
    unsafe {
        GetFileSecurityW(
            target.as_ptr(),
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            0,
            &mut needed,
        );
    }
    if needed == 0 {
        return Err(io::Error::last_os_error()).context("No se pudo consultar la ACL del archivo");
    }
    let mut descriptor = vec![0u32; (needed as usize).div_ceil(4)];
    // SAFETY: DWORD-aligned storage holds the requested security descriptor size.
    unsafe {
        if GetFileSecurityW(
            target.as_ptr(),
            DACL_SECURITY_INFORMATION,
            descriptor.as_mut_ptr().cast(),
            needed,
            &mut needed,
        ) == 0
        {
            return Err(io::Error::last_os_error()).context("No se pudo copiar la ACL del archivo");
        }
        // Do not merge broader parent permissions when the temporary is created.
        if SetSecurityDescriptorControl(
            descriptor.as_mut_ptr().cast(),
            SE_DACL_PROTECTED,
            SE_DACL_PROTECTED,
        ) == 0
        {
            return Err(io::Error::last_os_error())
                .context("No se pudo proteger la ACL del temporal");
        }
    }
    Ok(Some(descriptor))
}

struct Temporary {
    file: Option<File>,
    path: PathBuf,
    committed: bool,
}

impl Temporary {
    fn create(parent: &Path, descriptor: &mut Option<Vec<u32>>) -> Result<Self> {
        let mut random = [0u8; 16];
        // SAFETY: buffer is writable for exactly the supplied byte count; OS RNG selected.
        if unsafe {
            BCryptGenRandom(
                std::ptr::null_mut(),
                random.as_mut_ptr(),
                random.len() as u32,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        } < 0
        {
            bail!("No se pudo crear un nombre temporal seguro");
        }
        let suffix: String = random.iter().map(|b| format!("{b:02x}")).collect();
        let path = parent.join(format!(".codepeek-save-{suffix}.tmp"));
        let name = wide(&path);
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor
                .as_mut()
                .map_or(std::ptr::null_mut(), |d| d.as_mut_ptr().cast()),
            bInheritHandle: 0,
        };
        // SAFETY: all pointers remain valid during CreateFileW. CREATE_NEW prevents
        // collisions; share_mode=0 prevents peers from reading or replacing the temporary.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE | DELETE,
                0,
                &attributes,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error())
                .context("No se pudo crear el temporal protegido");
        }
        // SAFETY: successful CreateFileW transfers unique ownership to File.
        Ok(Self {
            file: Some(unsafe { File::from_raw_handle(handle) }),
            path,
            committed: false,
        })
    }

    fn commit(&mut self, target: &Path) -> Result<()> {
        if target.parent() != self.path.parent() {
            bail!("El reemplazo debe permanecer en el directorio del temporal");
        }
        // Keep a trailing NUL for the Win32 path conversion, even though the
        // FILE_RENAME_INFO byte length excludes it.
        let target = wide(target);
        let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
        let size = offset
            .checked_add(
                target
                    .len()
                    .checked_mul(2)
                    .context("Ruta demasiado larga")?,
            )
            .context("Ruta demasiado larga")?;
        let size = u32::try_from(size).context("Ruta demasiado larga")?;
        let mut storage = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
        // SAFETY: usize storage is aligned for FILE_RENAME_INFO, zero initialized,
        // and includes room for the complete trailing NUL-terminated UTF-16 filename.
        unsafe {
            let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
            (*info).Anonymous.ReplaceIfExists = true;
            (*info).FileNameLength = ((target.len() - 1) * 2) as u32;
            std::ptr::copy_nonoverlapping(
                target.as_ptr(),
                (*info).FileName.as_mut_ptr(),
                target.len(),
            );
            if SetFileInformationByHandle(
                self.file.as_ref().unwrap().as_raw_handle(),
                FileRenameInfo,
                info.cast(),
                size,
            ) == 0
            {
                return Err(io::Error::last_os_error())
                    .context("No se pudo sustituir el archivo; el original se conserva");
            }
        }
        self.committed = true;
        Ok(())
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        self.file.take(); // Close exclusive handle before removing an uncommitted file.
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub(super) fn save(target: &Path, content: &str) -> Result<()> {
    let target = std::path::absolute(target)?;
    let parent = target.parent().context("El destino no tiene directorio")?;
    let mut descriptor = security_descriptor(&target)?;
    let mut temporary = Temporary::create(parent, &mut descriptor)?;
    let file = temporary.file.as_mut().unwrap();
    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    temporary.commit(&target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Security::GetSecurityDescriptorDacl;

    fn dacl(path: &Path) -> Vec<u8> {
        let mut descriptor = security_descriptor(path).unwrap().unwrap();
        let (mut present, mut defaulted) = (0, 0);
        let mut acl = std::ptr::null_mut();
        // SAFETY: descriptor is owned and valid for both the query and bounded copy.
        unsafe {
            assert_ne!(
                GetSecurityDescriptorDacl(
                    descriptor.as_mut_ptr().cast(),
                    &mut present,
                    &mut acl,
                    &mut defaulted
                ),
                0
            );
            assert_ne!(present, 0);
            if acl.is_null() {
                return Vec::new();
            }
            std::slice::from_raw_parts(acl.cast::<u8>(), (*acl).AclSize as usize).to_vec()
        }
    }

    #[test]
    fn replacement_preserves_dacl_and_exclusive_temporary_identity() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("niño ' privado.txt");
        fs::write(&target, "before").unwrap();
        let before = dacl(&target);
        let mut descriptor = security_descriptor(&target).unwrap();
        let mut temporary = Temporary::create(dir.path(), &mut descriptor).unwrap();
        assert!(File::open(&temporary.path).is_err());
        assert!(fs::rename(&temporary.path, dir.path().join("substitute")).is_err());
        temporary
            .file
            .as_mut()
            .unwrap()
            .write_all(b"after")
            .unwrap();
        temporary.file.as_ref().unwrap().sync_all().unwrap();
        temporary.commit(&target).unwrap();
        drop(temporary);
        assert_eq!(fs::read_to_string(&target).unwrap(), "after");
        assert_eq!(dacl(&target), before);
    }

    #[test]
    fn failed_replacement_leaves_original_and_removes_temporary() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("original");
        fs::write(&target, "original").unwrap();
        let mut descriptor = security_descriptor(&target).unwrap();
        let mut temporary = Temporary::create(dir.path(), &mut descriptor).unwrap();
        temporary
            .file
            .as_mut()
            .unwrap()
            .write_all(b"changed")
            .unwrap();
        let temporary_path = temporary.path.clone();
        assert!(temporary.commit(dir.path()).is_err());
        drop(temporary);
        assert_eq!(fs::read_to_string(target).unwrap(), "original");
        assert!(!temporary_path.exists());
    }

    #[test]
    fn canonical_and_long_paths_save_in_the_original_directory() {
        let dir = tempfile::tempdir().unwrap();
        let mut parent = dir.path().canonicalize().unwrap();
        while parent.as_os_str().len() < 280 {
            parent.push("long-directory-component");
        }
        fs::create_dir_all(&parent).unwrap();
        let target = parent.join("niño.txt");
        fs::write(&target, "before").unwrap();
        save(&target, "after").unwrap();
        assert_eq!(fs::read_to_string(target).unwrap(), "after");
    }
}
