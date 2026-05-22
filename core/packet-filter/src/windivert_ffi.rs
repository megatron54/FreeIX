//! Raw FFI bindings for WinDivert loaded at runtime via libloading.

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::path::PathBuf;

use libloading::{Library, Symbol};
use tracing::info;

use crate::PacketFilterError;

/// WinDivert address structure.
/// We use an opaque byte array to avoid alignment/size mismatches across versions.
/// WinDivert 2.2 WINDIVERT_ADDRESS is 80 bytes but we over-allocate for safety.
#[repr(C, align(8))]
#[derive(Clone, Copy)]
pub struct WinDivertAddress {
    pub data: [u8; 128],
}

impl Default for WinDivertAddress {
    fn default() -> Self {
        Self { data: [0u8; 128] }
    }
}

type WinDivertOpenFn = unsafe extern "C" fn(
    filter: *const c_char,
    layer: c_int,
    priority: i16,
    flags: u64,
) -> *mut c_void;

type WinDivertRecvFn = unsafe extern "C" fn(
    handle: *mut c_void,
    packet: *mut u8,
    packet_len: u32,
    recv_len: *mut u32,
    addr: *mut WinDivertAddress,
) -> c_int;

type WinDivertSendFn = unsafe extern "C" fn(
    handle: *mut c_void,
    packet: *const u8,
    packet_len: u32,
    send_len: *mut u32,
    addr: *const WinDivertAddress,
) -> c_int;

type WinDivertCloseFn = unsafe extern "C" fn(handle: *mut c_void) -> c_int;

/// Loaded WinDivert library.
pub struct WinDivert {
    _lib: Library,
    open_fn: WinDivertOpenFn,
    recv_fn: WinDivertRecvFn,
    send_fn: WinDivertSendFn,
    close_fn: WinDivertCloseFn,
}

unsafe impl Send for WinDivert {}
unsafe impl Sync for WinDivert {}

impl WinDivert {
    /// Load WinDivert.dll from the application directory.
    pub fn load() -> Result<Self, PacketFilterError> {
        // Look for WinDivert.dll next to the exe
        let exe_dir = std::env::current_exe()
            .map(|p| p.parent().unwrap_or(&PathBuf::from(".")).to_path_buf())
            .unwrap_or_else(|_| PathBuf::from("."));

        let dll_path = exe_dir.join("WinDivert.dll");

        if !dll_path.exists() {
            return Err(PacketFilterError::LoadFailed(format!(
                "WinDivert.dll not found at {}",
                dll_path.display()
            )));
        }

        info!(path = %dll_path.display(), "Loading WinDivert.dll");

        let lib = unsafe {
            Library::new(&dll_path)
                .map_err(|e| PacketFilterError::LoadFailed(format!("{}", e)))?
        };

        let open_fn: WinDivertOpenFn;
        let recv_fn: WinDivertRecvFn;
        let send_fn: WinDivertSendFn;
        let close_fn: WinDivertCloseFn;

        unsafe {
            open_fn = *lib.get::<WinDivertOpenFn>(b"WinDivertOpen\0")
                .map_err(|e| PacketFilterError::LoadFailed(format!("WinDivertOpen: {}", e)))?;
            recv_fn = *lib.get::<WinDivertRecvFn>(b"WinDivertRecv\0")
                .map_err(|e| PacketFilterError::LoadFailed(format!("WinDivertRecv: {}", e)))?;
            send_fn = *lib.get::<WinDivertSendFn>(b"WinDivertSend\0")
                .map_err(|e| PacketFilterError::LoadFailed(format!("WinDivertSend: {}", e)))?;
            close_fn = *lib.get::<WinDivertCloseFn>(b"WinDivertClose\0")
                .map_err(|e| PacketFilterError::LoadFailed(format!("WinDivertClose: {}", e)))?;
        }

        Ok(Self {
            _lib: lib,
            open_fn,
            recv_fn,
            send_fn,
            close_fn,
        })
    }

    /// Open a WinDivert handle with the given filter.
    pub fn open(&self, filter: &str, layer: i32, priority: i16) -> Result<WinDivertHandle, PacketFilterError> {
        let c_filter = CString::new(filter)
            .map_err(|_| PacketFilterError::OpenFailed("invalid filter string".into()))?;

        let handle = unsafe {
            (self.open_fn)(c_filter.as_ptr(), layer as c_int, priority, 0)
        };

        // WinDivert returns INVALID_HANDLE_VALUE (0xFFFFFFFFFFFFFFFF) on failure
        let invalid: *mut c_void = -1isize as *mut c_void;
        if handle.is_null() || handle == invalid {
            let err = std::io::Error::last_os_error();
            return Err(PacketFilterError::OpenFailed(format!(
                "WinDivertOpen failed: {} (is app running as admin?)",
                err
            )));
        }

        Ok(WinDivertHandle {
            handle,
            recv_fn: self.recv_fn,
            send_fn: self.send_fn,
            close_fn: self.close_fn,
        })
    }
}

/// An open WinDivert handle.
pub struct WinDivertHandle {
    handle: *mut c_void,
    recv_fn: WinDivertRecvFn,
    send_fn: WinDivertSendFn,
    close_fn: WinDivertCloseFn,
}

unsafe impl Send for WinDivertHandle {}
unsafe impl Sync for WinDivertHandle {}

impl WinDivertHandle {
    /// Receive a diverted packet.
    pub fn recv(&self, buf: &mut [u8], addr: &mut WinDivertAddress) -> Result<usize, std::io::Error> {
        let mut recv_len: u32 = 0;
        let ret = unsafe {
            (self.recv_fn)(
                self.handle,
                buf.as_mut_ptr(),
                buf.len() as u32,
                &mut recv_len,
                addr,
            )
        };
        if ret == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(recv_len as usize)
        }
    }

    /// Re-inject a packet.
    pub fn send(&self, packet: &[u8], addr: &WinDivertAddress) -> Result<usize, std::io::Error> {
        let mut send_len: u32 = 0;
        let ret = unsafe {
            (self.send_fn)(
                self.handle,
                packet.as_ptr(),
                packet.len() as u32,
                &mut send_len,
                addr,
            )
        };
        if ret == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(send_len as usize)
        }
    }
}

impl Drop for WinDivertHandle {
    fn drop(&mut self) {
        unsafe {
            (self.close_fn)(self.handle);
        }
    }
}
