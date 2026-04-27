use std::mem;

use windows::Win32::Foundation::{CloseHandle, HANDLE, LUID};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueW, SE_DEBUG_NAME, SE_PRIVILEGE_ENABLED,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY, LUID_AND_ATTRIBUTES,
};
use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW, PROCESSENTRY32W,
    Process32FirstW, Process32NextW, TH32CS_SNAPMODULE, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
};

// KNOWN_ISSUE: EQ module base falls back to 0x140000000 when GetModuleBaseAddress
// returns 0 (e.g., TH32CS_SNAPMODULE unavailable without SeDebugPrivilege on first call).
// This matches C++ MemReader behavior.
const FALLBACK_BASE: u64 = 0x140000000;

// SAFETY: Win32 HANDLE values referencing kernel objects are valid across thread
// boundaries; the OS uses handle tables per-process, not per-thread.
unsafe impl Send for MemReader {}

#[derive(Debug)]
pub enum MemError {
    NoProcess,
    OpenFailed(u32),
    ReadFailed(u32),
}

impl std::fmt::Display for MemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemError::NoProcess => write!(f, "no process attached"),
            MemError::OpenFailed(e) => write!(f, "OpenProcess failed (error {e:#010x})"),
            MemError::ReadFailed(e) => write!(f, "ReadProcessMemory failed (error {e:#010x})"),
        }
    }
}

pub struct MemReader {
    pid: u32,
    handle: Option<HANDLE>,
    base_address: u64,
    original_filename: String,
    read_count: u32,
}

impl Default for MemReader {
    fn default() -> Self {
        Self::new()
    }
}

impl MemReader {
    pub fn new() -> Self {
        Self {
            pid: 0,
            handle: None,
            base_address: FALLBACK_BASE,
            original_filename: String::new(),
            read_count: 0,
        }
    }

    /// Request SE_DEBUG_NAME privilege so ReadProcessMemory works on protected processes.
    pub fn enable_debug_privileges() {
        unsafe {
            let mut token = HANDLE::default();
            if OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
                &mut token,
            )
            .is_err()
            {
                return;
            }

            let mut luid = LUID::default();
            if LookupPrivilegeValueW(None, SE_DEBUG_NAME, &mut luid).is_err() {
                let _ = CloseHandle(token);
                return;
            }

            let tp = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES {
                    Luid: luid,
                    Attributes: SE_PRIVILEGE_ENABLED,
                }],
            };
            let _ = AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None);
            let _ = CloseHandle(token);
        }
    }

    /// Find the first process whose exe name contains `name` (case-insensitive). Returns PID.
    pub fn find_process(name: &str) -> Option<u32> {
        let name_lower = name.to_lowercase();
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()? };
        let mut pe = PROCESSENTRY32W {
            dwSize: mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = None;
        unsafe {
            if Process32FirstW(snap, &mut pe).is_ok() {
                loop {
                    if wide_to_string(&pe.szExeFile)
                        .to_lowercase()
                        .contains(&name_lower)
                    {
                        found = Some(pe.th32ProcessID);
                        break;
                    }
                    if Process32NextW(snap, &mut pe).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
        }
        found
    }

    /// Open a process by PID and resolve its module base address.
    pub fn open(&mut self, pid: u32) -> Result<(), MemError> {
        self.close();
        let h = unsafe {
            OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, false, pid)
                .map_err(|e| MemError::OpenFailed(e.code().0 as u32))?
        };
        let filename = resolve_process_name(pid).unwrap_or_default();
        let base = get_module_base_address(pid, &filename).unwrap_or(FALLBACK_BASE);

        self.pid = pid;
        self.handle = Some(h);
        self.base_address = base;
        self.original_filename = filename.to_lowercase();
        self.read_count = 0;
        Ok(())
    }

    pub fn close(&mut self) {
        if let Some(h) = self.handle.take() {
            unsafe { let _ = CloseHandle(h); }
        }
        self.reset();
    }

    /// True if a process is attached and still alive (checked periodically via snapshot).
    pub fn is_valid(&mut self) -> bool {
        if self.pid == 0 {
            return false;
        }
        self.validate_process(false)
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn base_address(&self) -> u64 {
        self.base_address
    }

    /// Remap a canonical EQ address (0x140000000-based) to the actual loaded base address.
    /// Mirrors the `offset - 0x140000000 + getCurrentBaseAddress()` pattern used throughout
    /// the C++ NetworkServer handlers.
    pub fn canonical_to_actual(&self, addr: u64) -> u64 {
        addr.wrapping_sub(FALLBACK_BASE).wrapping_add(self.base_address)
    }

    /// Remap an actual address back to the canonical 0x140000000-based form for display.
    pub fn actual_to_canonical(&self, addr: u64) -> u64 {
        addr.wrapping_sub(self.base_address).wrapping_add(FALLBACK_BASE)
    }

    /// Find all processes whose exe name contains `name` (case-insensitive).
    /// Returns a list of (pid, exe_name) pairs.
    pub fn find_all_processes(name: &str) -> Vec<(u32, String)> {
        let name_lower = name.to_lowercase();
        let mut results = Vec::new();
        let snap = match unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) } {
            Ok(s) => s,
            Err(_) => return results,
        };
        let mut pe = PROCESSENTRY32W {
            dwSize: mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        unsafe {
            if Process32FirstW(snap, &mut pe).is_ok() {
                loop {
                    let exe = wide_to_string(&pe.szExeFile);
                    if exe.to_lowercase().contains(&name_lower) {
                        results.push((pe.th32ProcessID, exe));
                    }
                    if Process32NextW(snap, &mut pe).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
        }
        results
    }

    /// Read a `T`-sized value from the process at the given absolute virtual address.
    pub fn read<T: Copy>(&self, addr: u64) -> Result<T, MemError> {
        let h = self.handle.ok_or(MemError::NoProcess)?;
        let mut value = unsafe { mem::zeroed::<T>() };
        unsafe {
            ReadProcessMemory(
                h,
                addr as *const _,
                &mut value as *mut T as *mut _,
                mem::size_of::<T>(),
                None,
            )
            .map_err(|e| MemError::ReadFailed(e.code().0 as u32))?;
        }
        Ok(value)
    }

    /// Read a null-terminated string from an absolute address (at most `max_len` bytes).
    pub fn read_string(&self, addr: u64, max_len: usize) -> Result<String, MemError> {
        let buf = self.read_bytes(addr, max_len)?;
        Ok(parse_string_bytes(&buf))
    }

    /// Like `read_string` but returns an empty string unless the first character is alphanumeric.
    pub fn read_alnum_string(&self, addr: u64, max_len: usize) -> Result<String, MemError> {
        let s = self.read_string(addr, max_len)?;
        if s.chars().next().map_or(false, |c| c.is_alphanumeric()) {
            Ok(s)
        } else {
            Ok(String::new())
        }
    }

    /// Read a u64 pointer from an absolute address.
    pub fn read_pointer(&self, addr: u64) -> Result<u64, MemError> {
        self.read::<u64>(addr)
    }

    /// Read a u64 pointer with canonical-to-actual base remapping
    /// (0x140000000 → `self.base_address`).
    pub fn read_raw_pointer(&self, addr: u64) -> Result<u64, MemError> {
        let remapped = addr - FALLBACK_BASE + self.base_address;
        self.read::<u64>(remapped)
    }

    /// Read `size` bytes from an absolute address, clamping to the enclosing memory region
    /// to prevent partial-region reads from returning nothing.
    pub fn read_bytes(&self, addr: u64, size: usize) -> Result<Vec<u8>, MemError> {
        let h = self.handle.ok_or(MemError::NoProcess)?;
        if size == 0 {
            return Ok(Vec::new());
        }
        let clamped = unsafe { clamp_to_region(h, addr, size) };
        if clamped == 0 {
            return Ok(vec![0u8; size]);
        }
        let mut buf = vec![0u8; clamped];
        let ok = unsafe {
            ReadProcessMemory(h, addr as *const _, buf.as_mut_ptr() as *mut _, clamped, None)
                .is_ok()
        };
        if !ok {
            buf.fill(0);
        }
        if clamped < size {
            buf.resize(size, 0);
        }
        Ok(buf)
    }

    fn reset(&mut self) {
        self.pid = 0;
        self.handle = None;
        self.base_address = FALLBACK_BASE;
        self.original_filename.clear();
        self.read_count = 0;
    }

    fn validate_process(&mut self, force: bool) -> bool {
        self.read_count = (self.read_count + 1) % 100;
        if !force && self.read_count != 2 {
            return true;
        }
        let alive = check_process_alive(self.pid, &self.original_filename);
        if !alive {
            self.close();
        }
        alive
    }
}

impl Drop for MemReader {
    fn drop(&mut self) {
        self.close();
    }
}

// --- Module-private helpers ---
// All unsafe Win32 calls are contained here; callers see a safe interface.

pub(crate) fn parse_string_bytes(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

fn wide_to_string(wide: &[u16]) -> String {
    let end = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..end])
}

fn resolve_process_name(pid: u32) -> Option<String> {
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()? };
    let mut pe = PROCESSENTRY32W {
        dwSize: mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut name = None;
    unsafe {
        if Process32FirstW(snap, &mut pe).is_ok() {
            loop {
                if pe.th32ProcessID == pid {
                    name = Some(wide_to_string(&pe.szExeFile));
                    break;
                }
                if Process32NextW(snap, &mut pe).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    name
}

fn get_module_base_address(pid: u32, exe_name: &str) -> Option<u64> {
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, pid).ok()? };
    let mut me = MODULEENTRY32W {
        dwSize: mem::size_of::<MODULEENTRY32W>() as u32,
        ..Default::default()
    };
    let mut base = None;
    unsafe {
        if Module32FirstW(snap, &mut me).is_ok() {
            loop {
                if wide_to_string(&me.szModule).eq_ignore_ascii_case(exe_name) {
                    base = Some(me.modBaseAddr as u64);
                    break;
                }
                if Module32NextW(snap, &mut me).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    base
}

fn check_process_alive(pid: u32, expected_name: &str) -> bool {
    let snap = match unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) } {
        Ok(s) => s,
        Err(_) => return false,
    };
    let mut pe = PROCESSENTRY32W {
        dwSize: mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut found = false;
    unsafe {
        if Process32FirstW(snap, &mut pe).is_ok() {
            loop {
                if pe.th32ProcessID == pid {
                    found = wide_to_string(&pe.szExeFile).to_lowercase() == expected_name;
                    break;
                }
                if Process32NextW(snap, &mut pe).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    found
}

/// Clamp `requested` to the bytes remaining in the VirtualQueryEx region starting at `addr`.
/// Returns `requested` unchanged if VirtualQueryEx fails (let ReadProcessMemory handle it).
unsafe fn clamp_to_region(handle: HANDLE, addr: u64, requested: usize) -> usize {
    let mut info = MEMORY_BASIC_INFORMATION::default();
    let ret = unsafe {
        VirtualQueryEx(
            handle,
            Some(addr as *const _),
            &mut info,
            mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    if ret == 0 {
        return requested;
    }
    let bytes_into_region = addr.saturating_sub(info.BaseAddress as u64) as usize;
    let bytes_remaining = (info.RegionSize as usize).saturating_sub(bytes_into_region);
    requested.min(bytes_remaining)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_string_bytes_null_terminated() {
        let buf = b"Surefall\x00garbage";
        assert_eq!(parse_string_bytes(buf), "Surefall");
    }

    #[test]
    fn parse_string_bytes_no_null() {
        let buf = b"hello";
        assert_eq!(parse_string_bytes(buf), "hello");
    }

    #[test]
    fn parse_string_bytes_leading_null() {
        let buf = b"\x00hello";
        assert_eq!(parse_string_bytes(buf), "");
    }

    #[test]
    fn new_reader_has_default_state() {
        let r = MemReader::new();
        assert_eq!(r.pid(), 0);
        assert_eq!(r.base_address(), FALLBACK_BASE);
    }

    #[test]
    fn find_nonexistent_process_returns_none() {
        assert!(MemReader::find_process("__no_such_process_xyz__").is_none());
    }

    #[test]
    fn read_on_detached_reader_returns_error() {
        let r = MemReader::new();
        assert!(matches!(r.read::<u64>(0x1000), Err(MemError::NoProcess)));
    }
}