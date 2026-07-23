#![allow(dead_code)]

use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const LOCK_EX: i32 = 2;
const LOCK_NB: i32 = 4;
const SOL_SOCKET: i32 = 1;
const SO_PEERCRED: i32 = 17;
const O_DIRECTORY: i32 = 0o200000;
const O_NOFOLLOW: i32 = 0o400000;
const O_CLOEXEC: i32 = 0o2000000;
const O_RDONLY: i32 = 0;
const RENAME_NOREPLACE: u32 = 1;
static QUARANTINE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub(crate) enum RuntimeError {
    Unsafe,
    DaemonRunning,
}

impl RuntimeError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Unsafe => "config.invalid",
            Self::DaemonRunning => "lifecycle.daemon_running",
        }
    }

    pub(crate) fn message(&self) -> &'static str {
        match self {
            Self::Unsafe => "invalid runtime path",
            Self::DaemonRunning => "daemon already running",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Paths {
    pub(crate) socket: PathBuf,
    directories: Vec<PathBuf>,
}

pub(crate) struct Guard {
    directory: File,
    _lock: File,
    listener_identity: Option<Identity>,
    descriptor_proof: Option<Identity>,
    pathname_identity: Option<Identity>,
    cleanup_identity: Option<Identity>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Identity {
    device: u64,
    inode: u64,
}

#[repr(C)]
struct UCred {
    pid: i32,
    uid: u32,
    gid: u32,
}

unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
    fn fchmod(fd: i32, mode: u32) -> i32;
    fn dup(fd: i32) -> i32;
    fn renameat2(oldfd: i32, oldpath: *const i8, newfd: i32, newpath: *const i8, flags: u32)
        -> i32;
    fn geteuid() -> u32;
    fn umask(mask: u32) -> u32;
    fn open(path: *const i8, flags: i32, mode: i32) -> i32;
    fn openat(dirfd: i32, path: *const i8, flags: i32, mode: i32) -> i32;
    fn mkdirat(dirfd: i32, path: *const i8, mode: u32) -> i32;
    fn getsockopt(fd: i32, level: i32, option: i32, value: *mut UCred, length: *mut u32) -> i32;
}

pub(crate) fn probe() -> Result<Paths, RuntimeError> {
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) {
        if runtime.is_absolute() {
            if trusted_ancestors(&runtime) && xdg_base_is_safe(&runtime) {
                let child = runtime.join("akashic");
                if fs::symlink_metadata(&child).is_ok() && !private_dir_is_safe(&child) {
                    return Err(RuntimeError::Unsafe);
                }
                return Ok(Paths {
                    socket: child.join("daemon.sock"),
                    directories: vec![runtime.clone(), child],
                });
            }
            return fallback_paths();
        }
    }
    fallback_paths()
}

fn fallback_paths() -> Result<Paths, RuntimeError> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".local/state"))
        })
        .or_else(|| std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from))
        .ok_or(RuntimeError::Unsafe)?;
    if !state.is_absolute() || !base_is_safe(&state) {
        return Err(RuntimeError::Unsafe);
    }
    let akashic = state.join("akashic");
    let run = akashic.join("run");
    for directory in [&akashic, &run] {
        if !private_dir_is_safe(directory) {
            return Err(RuntimeError::Unsafe);
        }
    }
    Ok(Paths {
        socket: run.join("daemon.sock"),
        directories: vec![state, akashic, run],
    })
}

fn xdg_base_is_safe(runtime: &Path) -> bool {
    match fs::symlink_metadata(runtime) {
        Ok(metadata) => {
            !metadata.file_type().is_symlink()
                && metadata.is_dir()
                && metadata.uid() == current_uid()
                && metadata.permissions().mode() & 0o777 == 0o700
                && metadata.permissions().mode() & 0o7000 == 0
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(_) => false,
    }
}

fn base_is_safe(path: &Path) -> bool {
    trusted_ancestors(path)
        && (match fs::symlink_metadata(path) {
            Ok(metadata) => {
                !metadata.file_type().is_symlink()
                    && metadata.is_dir()
                    && metadata.uid() == current_uid()
                    && metadata.permissions().mode() & 0o777 == 0o700
                    && metadata.permissions().mode() & 0o7000 == 0
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => true,
            Err(_) => false,
        })
}

fn private_dir_is_safe(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            !metadata.file_type().is_symlink()
                && metadata.is_dir()
                && metadata.uid() == current_uid()
                && metadata.permissions().mode() & 0o777 == 0o700
                && metadata.permissions().mode() & 0o7000 == 0
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => trusted_ancestors(path),
        Err(_) => false,
    }
}

fn trusted_ancestors(path: &Path) -> bool {
    let mut current = path.parent();
    while let Some(path) = current {
        match fs::symlink_metadata(path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink()
                    || (metadata.uid() != 0 && metadata.uid() != current_uid())
                    || metadata.permissions().mode() & 0o6000 != 0
                    || (metadata.permissions().mode() & 0o022 != 0
                        && metadata.permissions().mode() & 0o1000 == 0)
                {
                    return false;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return false,
        }
        current = path.parent();
    }
    true
}

fn current_uid() -> u32 {
    unsafe { geteuid() }
}

fn ensure_directories(paths: &Paths) -> Result<(), RuntimeError> {
    for directory in &paths.directories {
        let _ = open_directory_chain(directory, true)?;
    }
    Ok(())
}

pub(crate) fn artifact_store_path() -> Result<PathBuf, RuntimeError> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .ok_or(RuntimeError::Unsafe)?;
    if !state.is_absolute() {
        return Err(RuntimeError::Unsafe);
    }
    let _directory = open_directory_chain(&state.join("akashic"), true)?;
    Ok(state.join("akashic/artifacts.sqlite"))
}

fn open_directory(path: &Path) -> Result<File, RuntimeError> {
    open_directory_chain(path, false)
}

fn open_directory_chain(path: &Path, create: bool) -> Result<File, RuntimeError> {
    if !path.is_absolute() {
        return Err(RuntimeError::Unsafe);
    }
    let root = CString::new("/").map_err(|_| RuntimeError::Unsafe)?;
    let fd = unsafe {
        open(
            root.as_ptr(),
            O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
            0,
        )
    };
    if fd < 0 {
        return Err(RuntimeError::Unsafe);
    }
    let mut directory = unsafe { File::from_raw_fd(fd) };
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(component) => components.push(component),
            Component::RootDir => {}
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                return Err(RuntimeError::Unsafe)
            }
        }
    }
    for (index, component) in components.iter().enumerate() {
        let final_component = index + 1 == components.len();
        let name = CString::new(component.as_bytes()).map_err(|_| RuntimeError::Unsafe)?;
        let mut child = open_at_directory(&directory, &name);
        if child.is_err() && create && io::Error::last_os_error().kind() == io::ErrorKind::NotFound
        {
            if unsafe { mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
                return Err(RuntimeError::Unsafe);
            }
            child = open_at_directory(&directory, &name);
        }
        let child = child?;
        let metadata = child.metadata().map_err(|_| RuntimeError::Unsafe)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || metadata.permissions().mode() & 0o6000 != 0
        {
            return Err(RuntimeError::Unsafe);
        }
        if final_component {
            if metadata.uid() != current_uid() || metadata.permissions().mode() & 0o777 != 0o700 {
                return Err(RuntimeError::Unsafe);
            }
        } else if (metadata.uid() != 0 && metadata.uid() != current_uid())
            || (metadata.permissions().mode() & 0o022 != 0
                && metadata.permissions().mode() & 0o1000 == 0)
        {
            return Err(RuntimeError::Unsafe);
        }
        directory = child;
    }
    Ok(directory)
}

fn open_at_directory(parent: &File, name: &CString) -> Result<File, RuntimeError> {
    let fd = unsafe {
        openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
            0,
        )
    };
    if fd < 0 {
        return Err(RuntimeError::Unsafe);
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn anchored_child(directory: &File, name: &str) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}/{}", directory.as_raw_fd(), name))
}

pub(crate) fn acquire() -> Result<Guard, RuntimeError> {
    unsafe { umask(0o077) };
    let paths = probe()?;
    ensure_directories(&paths)?;
    let directory = open_directory(paths.directories.last().ok_or(RuntimeError::Unsafe)?)?;
    let lock_path = anchored_child(&directory, "daemon.lock");
    match fs::symlink_metadata(&lock_path) {
        Ok(metadata)
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || metadata.uid() != current_uid()
                || metadata.permissions().mode() & 0o777 != 0o600
                || metadata.permissions().mode() & 0o7000 != 0 =>
        {
            return Err(RuntimeError::Unsafe);
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err(RuntimeError::Unsafe),
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW)
        .open(&lock_path)
        .map_err(|_| RuntimeError::Unsafe)?;
    let metadata = lock.metadata().map_err(|_| RuntimeError::Unsafe)?;
    if metadata.uid() != current_uid()
        || metadata.permissions().mode() & 0o777 != 0o600
        || metadata.permissions().mode() & 0o7000 != 0
    {
        return Err(RuntimeError::Unsafe);
    }
    let result = unsafe { flock(lock.as_raw_fd(), LOCK_EX | LOCK_NB) };
    if result != 0 {
        return Err(
            if io::Error::last_os_error()
                .raw_os_error()
                .is_some_and(|error| error == 11)
            {
                RuntimeError::DaemonRunning
            } else {
                RuntimeError::Unsafe
            },
        );
    }
    if unsafe { fchmod(lock.as_raw_fd(), 0o600) } != 0 {
        return Err(RuntimeError::Unsafe);
    }
    Ok(Guard {
        directory,
        _lock: lock,
        listener_identity: None,
        descriptor_proof: None,
        pathname_identity: None,
        cleanup_identity: None,
    })
}

impl Guard {
    pub(crate) fn bind(&mut self) -> Result<UnixListener, RuntimeError> {
        let socket = anchored_child(&self.directory, "daemon.sock");
        match fs::symlink_metadata(&socket) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink()
                    || !metadata.file_type().is_socket()
                    || metadata.uid() != current_uid()
                    || metadata.permissions().mode() & 0o777 != 0o600
                    || metadata.permissions().mode() & 0o7000 != 0
                {
                    return Err(RuntimeError::Unsafe);
                }
                let before = identity(&metadata);
                match UnixStream::connect(&socket) {
                    Ok(_) => return Err(RuntimeError::DaemonRunning),
                    Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
                        let Some(after) = socket_identity(&socket) else {
                            return Err(RuntimeError::Unsafe);
                        };
                        if before != after {
                            return Err(RuntimeError::Unsafe);
                        }
                        return Err(RuntimeError::Unsafe);
                    }
                    Err(_) => return Err(RuntimeError::Unsafe),
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err(RuntimeError::Unsafe),
        }
        unsafe { umask(0o177) };
        let listener_result = UnixListener::bind(&socket);
        unsafe { umask(0o077) };
        let listener = listener_result.map_err(|_| RuntimeError::Unsafe)?;
        // Linux exposes AF_UNIX fstat and directory-entry identities in
        // different namespaces; retain both proofs and never trust either alone.
        let descriptor_proof = Some(descriptor_identity(listener.as_raw_fd())?);
        let pathname_before = socket_identity(&socket);
        if !ownership_proven(descriptor_proof, pathname_before) {
            return Err(RuntimeError::Unsafe);
        }
        if unsafe { fchmod(listener.as_raw_fd(), 0o600) } != 0 {
            return Err(RuntimeError::Unsafe);
        }
        let descriptor_metadata = descriptor_metadata(listener.as_raw_fd())?;
        if descriptor_metadata.permissions().mode() & 0o777 != 0o600
            || descriptor_metadata.permissions().mode() & 0o7000 != 0
        {
            return Err(RuntimeError::Unsafe);
        }
        let pathname_after = socket_identity(&socket);
        if !pathname_proof_stable(pathname_before, pathname_after) {
            return Err(RuntimeError::Unsafe);
        }
        self.descriptor_proof = descriptor_proof;
        self.pathname_identity = pathname_after;
        self.listener_identity = pathname_after;
        self.cleanup_identity = pathname_after;
        Ok(listener)
    }

    pub(crate) fn cleanup(&self) -> Result<(), RuntimeError> {
        let socket = anchored_child(&self.directory, "daemon.sock");
        let Some(proof) = self.descriptor_proof else {
            return Err(RuntimeError::Unsafe);
        };
        let Some(listener) = self.listener_identity else {
            return Err(RuntimeError::Unsafe);
        };
        let Some(pathname) = self.pathname_identity else {
            return Err(RuntimeError::Unsafe);
        };
        let Some(expected) = self.cleanup_identity else {
            return Err(RuntimeError::Unsafe);
        };
        let Some(actual) = socket_identity(&socket) else {
            return Err(RuntimeError::Unsafe);
        };
        if proof.device == 0
            || proof.inode == 0
            || listener != pathname
            || pathname != expected
            || expected != actual
        {
            return Err(RuntimeError::Unsafe);
        }
        atomic_cleanup(&self.directory, expected)
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

fn identity(metadata: &fs::Metadata) -> Identity {
    Identity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

fn descriptor_identity(fd: i32) -> Result<Identity, RuntimeError> {
    Ok(identity(&descriptor_metadata(fd)?))
}

fn ownership_proven(descriptor: Option<Identity>, pathname: Option<Identity>) -> bool {
    descriptor.is_some_and(|identity| identity.device != 0 && identity.inode != 0)
        && pathname.is_some_and(|identity| identity.device != 0 && identity.inode != 0)
}

fn pathname_proof_stable(before: Option<Identity>, after: Option<Identity>) -> bool {
    before.is_some() && before == after
}

fn descriptor_metadata(fd: i32) -> Result<fs::Metadata, RuntimeError> {
    let duplicate = unsafe { dup(fd) };
    if duplicate < 0 {
        return Err(RuntimeError::Unsafe);
    }
    let file = unsafe { File::from_raw_fd(duplicate) };
    file.metadata().map_err(|_| RuntimeError::Unsafe)
}

fn atomic_cleanup(directory: &File, expected: Identity) -> Result<(), RuntimeError> {
    let sequence = QUARANTINE_ID.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let name = CString::new(format!(
        ".daemon.sock.quarantine.{}.{}.{}",
        std::process::id(),
        timestamp,
        sequence
    ))
    .map_err(|_| RuntimeError::Unsafe)?;
    let socket = CString::new("daemon.sock").map_err(|_| RuntimeError::Unsafe)?;
    if unsafe {
        renameat2(
            directory.as_raw_fd(),
            socket.as_ptr(),
            directory.as_raw_fd(),
            name.as_ptr(),
            RENAME_NOREPLACE,
        )
    } != 0
    {
        return Err(RuntimeError::Unsafe);
    }
    let quarantine = anchored_child(directory, name.to_str().unwrap_or_default());
    if socket_identity(&quarantine) != Some(expected) {
        let _ = unsafe {
            renameat2(
                directory.as_raw_fd(),
                name.as_ptr(),
                directory.as_raw_fd(),
                socket.as_ptr(),
                RENAME_NOREPLACE,
            )
        };
        return Err(RuntimeError::Unsafe);
    }
    // The owned socket is retained in private quarantine: Linux has no
    // conditional unlink-by-inode primitive, so never unlink after checking.
    Ok(())
}

fn socket_identity(path: &Path) -> Option<Identity> {
    fs::symlink_metadata(path)
        .ok()
        .map(|metadata| identity(&metadata))
}

pub(crate) fn authorize_peer(stream: &UnixStream) -> Result<(), RuntimeError> {
    let mut credentials = UCred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<UCred>() as u32;
    let result = unsafe {
        getsockopt(
            stream.as_raw_fd(),
            SOL_SOCKET,
            SO_PEERCRED,
            &mut credentials,
            &mut length,
        )
    };
    if result == 0 && credentials.uid == current_uid() {
        Ok(())
    } else {
        Err(RuntimeError::Unsafe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn cleanup_does_not_remove_a_replacement_with_a_different_identity() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("runtime-cleanup-{stamp}"));
        fs::create_dir_all(&directory_path).expect("directory");
        fs::set_permissions(&directory_path, fs::Permissions::from_mode(0o700))
            .expect("directory mode");
        let directory = open_directory(&directory_path).expect("directory fd");
        let socket = directory_path.join("daemon.sock");
        let replacement = directory_path.join("replacement");
        fs::write(&socket, b"old").expect("old socket");
        let old_identity = socket_identity(&socket).expect("old identity");
        fs::write(&replacement, b"new").expect("replacement");
        fs::rename(&replacement, &socket).expect("replace socket");
        let guard = Guard {
            directory,
            _lock: File::open("/dev/null").expect("lock fixture"),
            listener_identity: Some(old_identity),
            descriptor_proof: Some(old_identity),
            pathname_identity: Some(old_identity),
            cleanup_identity: Some(old_identity),
        };
        let _ = guard.cleanup();
        assert!(socket.exists());
    }

    #[test]
    fn cleanup_does_not_remove_when_descriptor_and_path_ownership_differ() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("runtime-cleanup-mismatch-{stamp}"));
        fs::create_dir_all(&directory_path).expect("directory");
        fs::set_permissions(&directory_path, fs::Permissions::from_mode(0o700))
            .expect("directory mode");
        let directory = open_directory(&directory_path).expect("directory fd");
        let socket = directory_path.join("daemon.sock");
        fs::write(&socket, b"replacement").expect("socket");
        let path_identity = socket_identity(&socket).expect("path identity");
        let guard = Guard {
            directory,
            _lock: File::open("/dev/null").expect("lock fixture"),
            listener_identity: Some(Identity {
                device: path_identity.device + 1,
                inode: path_identity.inode,
            }),
            descriptor_proof: Some(Identity {
                device: path_identity.device + 1,
                inode: path_identity.inode,
            }),
            pathname_identity: Some(path_identity),
            cleanup_identity: Some(path_identity),
        };
        let _ = guard.cleanup();
        assert!(socket.exists());
    }

    #[test]
    fn created_socket_identity_comes_from_the_bound_descriptor() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("runtime-identity-{stamp}.sock"));
        let listener = UnixListener::bind(&path).expect("socket");
        let from_descriptor =
            descriptor_identity(listener.as_raw_fd()).expect("descriptor identity");
        let duplicate = unsafe { dup(listener.as_raw_fd()) };
        let duplicate_identity = descriptor_identity(duplicate).expect("duplicate identity");
        assert_eq!(from_descriptor, duplicate_identity);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn ownership_requires_both_descriptor_and_path_proofs() {
        assert!(!ownership_proven(
            None,
            Some(Identity {
                device: 1,
                inode: 1
            })
        ));
        assert!(!ownership_proven(
            Some(Identity {
                device: 0,
                inode: 1
            }),
            Some(Identity {
                device: 1,
                inode: 1
            })
        ));
        assert!(ownership_proven(
            Some(Identity {
                device: 1,
                inode: 1
            }),
            Some(Identity {
                device: 1,
                inode: 1
            })
        ));
        assert!(!pathname_proof_stable(
            Some(Identity {
                device: 1,
                inode: 1
            }),
            Some(Identity {
                device: 2,
                inode: 1
            }),
        ));
    }
}
