//! macOS vnode notifications for exactly one session file and its parent, or
//! for a set of paths its reader changes as it learns what to follow
//! ([`SetWatch`]). Descriptor watches register synchronously and do not depend
//! on the FSEvents service.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io;
use std::net::Shutdown;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::SyncSender,
};

pub(super) struct NativeWatch {
    stop: UnixStream,
    failed: Arc<AtomicBool>,
}

impl NativeWatch {
    pub(super) fn start(parent: &Path, path: PathBuf, signal: SyncSender<()>) -> io::Result<Self> {
        let queue = new_queue()?;
        let (stop, stopped) = UnixStream::pair()?;
        let parent = event_file(parent)?;
        register(
            &queue,
            parent.as_raw_fd(),
            libc::EVFILT_VNODE,
            vnode_flags(),
        )?;
        register(&queue, stopped.as_raw_fd(), libc::EVFILT_READ, 0)?;
        let file = arm_file(&queue, &path)?;
        let failed = Arc::new(AtomicBool::new(false));
        let worker_failed = failed.clone();
        std::thread::spawn(move || run(queue, parent, file, stopped, path, signal, worker_failed));
        Ok(Self { stop, failed })
    }

    pub(super) fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
}

impl Drop for NativeWatch {
    fn drop(&mut self) {
        // EOF wakes EVFILT_READ; teardown never joins a filesystem thread on the UI loop.
        let _ = self.stop.shutdown(Shutdown::Both);
    }
}

/// Vnode notifications for a set of files and directories: one queue, a thread
/// that wakes the reader on any change to any of them, and the descriptors
/// [`Self::set`] arms. A directory's own vnode changes when an entry is added
/// or removed, not when a file in it grows, so each file followed is armed
/// itself.
pub(super) struct SetWatch {
    queue: Arc<OwnedFd>,
    stop: UnixStream,
    failed: Arc<AtomicBool>,
    armed: HashMap<PathBuf, File>,
}

impl SetWatch {
    pub(super) fn start(signal: SyncSender<()>) -> io::Result<Self> {
        let queue = Arc::new(new_queue()?);
        let (stop, stopped) = UnixStream::pair()?;
        register(&queue, stopped.as_raw_fd(), libc::EVFILT_READ, 0)?;
        let failed = Arc::new(AtomicBool::new(false));
        let worker_queue = queue.clone();
        let worker_failed = failed.clone();
        std::thread::spawn(move || run_set(&worker_queue, &stopped, &signal, &worker_failed));
        Ok(Self {
            queue,
            stop,
            failed,
            armed: HashMap::new(),
        })
    }

    pub(super) fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    /// Watch `paths` from now on: arm each that exists and isn't armed, re-arm
    /// one replaced since, and drop the rest; closing a descriptor takes it off
    /// the queue. Whether it armed any, whose changes before now only a fresh
    /// read sees.
    pub(super) fn set(&mut self, paths: &[PathBuf]) -> bool {
        self.armed
            .retain(|path, file| paths.contains(path) && same_inode(path, file));
        let mut armed = false;
        for path in paths {
            if self.armed.contains_key(path) {
                continue;
            }
            let Ok(file) = event_file(path) else {
                continue;
            };
            if register(
                &self.queue,
                file.as_raw_fd(),
                libc::EVFILT_VNODE,
                vnode_flags(),
            )
            .is_ok()
            {
                self.armed.insert(path.clone(), file);
                armed = true;
            }
        }
        armed
    }
}

impl Drop for SetWatch {
    fn drop(&mut self) {
        // EOF wakes EVFILT_READ; teardown never joins a filesystem thread.
        let _ = self.stop.shutdown(Shutdown::Both);
    }
}

/// Whether `file` is still what `path` names.
fn same_inode(path: &Path, file: &File) -> bool {
    match (std::fs::symlink_metadata(path), file.metadata()) {
        (Ok(now), Ok(armed)) => now.dev() == armed.dev() && now.ino() == armed.ino(),
        _ => false,
    }
}

fn new_queue() -> io::Result<OwnedFd> {
    // SAFETY: kqueue returns a new descriptor owned solely by its caller.
    let raw = unsafe { libc::kqueue() };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful kqueue returned a valid, uniquely owned descriptor.
    let queue = unsafe { OwnedFd::from_raw_fd(raw) };
    // SAFETY: the descriptor is valid; F_SETFD changes only its inheritance flag.
    if unsafe { libc::fcntl(raw, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(queue)
}

/// Wake the reader on any change to anything armed, until `stopped` closes.
fn run_set(queue: &OwnedFd, stopped: &UnixStream, signal: &SyncSender<()>, failed: &AtomicBool) {
    loop {
        // SAFETY: zero is valid for every scalar/pointer field. kevent initializes
        // exactly the returned number of entries before they are inspected.
        let mut events: [libc::kevent; 8] = unsafe { std::mem::zeroed() };
        // SAFETY: the event buffer holds eight entries and remains live for the
        // syscall. A null timeout blocks until a kernel event, never on a timer.
        let count = unsafe {
            libc::kevent(
                queue.as_raw_fd(),
                std::ptr::null(),
                0,
                events.as_mut_ptr(),
                events.len() as i32,
                std::ptr::null(),
            )
        };
        if count < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            failed.store(true, Ordering::Release);
            let _ = signal.try_send(());
            return;
        }
        let events = &events[..count as usize];
        if events
            .iter()
            .any(|event| event.ident == stopped.as_raw_fd() as libc::uintptr_t)
        {
            return;
        }
        if !events.is_empty() {
            let _ = signal.try_send(());
        }
    }
}

fn event_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_EVTONLY | libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
}

const fn vnode_flags() -> u32 {
    libc::NOTE_WRITE
        | libc::NOTE_EXTEND
        | libc::NOTE_ATTRIB
        | libc::NOTE_RENAME
        | libc::NOTE_DELETE
        | libc::NOTE_REVOKE
}

fn register(queue: &OwnedFd, fd: RawFd, filter: i16, flags: u32) -> io::Result<()> {
    let event = libc::kevent {
        ident: fd as libc::uintptr_t,
        filter,
        flags: libc::EV_ADD | libc::EV_CLEAR,
        fflags: flags,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    // SAFETY: the queue and watched descriptors are live. One initialized change
    // is supplied; no output events or timeout pointers are dereferenced.
    if unsafe {
        libc::kevent(
            queue.as_raw_fd(),
            &raw const event,
            1,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
        )
    } < 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn arm_file(queue: &OwnedFd, path: &Path) -> io::Result<Option<File>> {
    match event_file(path) {
        Ok(file) => {
            if !file.metadata()?.is_file() {
                return Ok(None);
            }
            register(queue, file.as_raw_fd(), libc::EVFILT_VNODE, vnode_flags())?;
            Ok(Some(file))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn run(
    queue: OwnedFd,
    parent: File,
    mut file: Option<File>,
    stopped: UnixStream,
    path: PathBuf,
    signal: SyncSender<()>,
    failed: Arc<AtomicBool>,
) {
    loop {
        // SAFETY: zero is valid for every scalar/pointer field. kevent initializes
        // exactly the returned number of entries before they are inspected.
        let mut events: [libc::kevent; 4] = unsafe { std::mem::zeroed() };
        // SAFETY: the event buffer holds four entries and remains live for the
        // syscall. A null timeout blocks until a kernel event, never on a timer.
        let count = unsafe {
            libc::kevent(
                queue.as_raw_fd(),
                std::ptr::null(),
                0,
                events.as_mut_ptr(),
                events.len() as i32,
                std::ptr::null(),
            )
        };
        if count < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            failed.store(true, Ordering::Release);
            let _ = signal.try_send(());
            return;
        }
        for event in &events[..count as usize] {
            if event.ident == stopped.as_raw_fd() as libc::uintptr_t {
                return;
            }
            if event.flags & libc::EV_ERROR != 0
                || (event.ident == parent.as_raw_fd() as libc::uintptr_t
                    && event.fflags & (libc::NOTE_DELETE | libc::NOTE_REVOKE | libc::NOTE_RENAME)
                        != 0)
            {
                failed.store(true, Ordering::Release);
                let _ = signal.try_send(());
                return;
            }
            let changed_file = file
                .as_ref()
                .is_some_and(|file| event.ident == file.as_raw_fd() as libc::uintptr_t);
            if !changed_file
                || event.fflags & (libc::NOTE_DELETE | libc::NOTE_RENAME | libc::NOTE_REVOKE) != 0
            {
                // Arm the new inode before waking the reader. Writes racing the
                // replacement are covered either by this watch or its first read.
                file = None;
                match arm_file(&queue, &path) {
                    Ok(rebound) => file = rebound,
                    Err(_) => {
                        failed.store(true, Ordering::Release);
                    }
                }
            }
            let _ = signal.try_send(());
        }
    }
}
