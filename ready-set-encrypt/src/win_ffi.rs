//! Win32 FFI surface used by the AppContainer-based Windows sandbox backend.
//!
//! Every Win32 API the launcher binary calls is declared here as a
//! `pub fn` inside an `extern "system"` block, grouped by source DLL.
//! Type aliases (`HANDLE`, `LPCWSTR`, etc.) mirror the layout used by
//! `windows-sys` so a future swap to that crate is search-and-replace.
//!
//! This file is the canonical FFI contract: the set of `pub fn`
//! declarations defines exactly which Win32 APIs the crate depends on,
//! reviewable as a single artifact during PR review. Adding a new API
//! means appending a single `pub fn` line to the relevant linked block.

#![allow(unsafe_code)]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(missing_docs)]
#![allow(dead_code)]
// Doc comments routinely reference Win32 type / API names that look like
// items but aren't reachable Rust paths (`HANDLE`, `STARTUPINFOEXW`,
// `CreateProcessW`, ...). Rather than backtick every single one, broadly
// allow the doc-markdown lint inside this FFI-shim file.
#![allow(clippy::doc_markdown)]

// Opaque pointer / handle aliases. Mirror the windows-sys layout so a
// future swap is a search-and-replace.
pub type BOOL = i32;
pub type DWORD = u32;
pub type WORD = u16;
pub type HANDLE = *mut core::ffi::c_void;
pub type HLOCAL = *mut core::ffi::c_void;
pub type PVOID = *mut core::ffi::c_void;
pub type PCVOID = *const core::ffi::c_void;
pub type LPWSTR = *mut u16;
pub type LPCWSTR = *const u16;
pub type LPBYTE = *mut u8;
pub type PSID = *mut core::ffi::c_void;
pub type PSECURITY_DESCRIPTOR = *mut core::ffi::c_void;
pub type LPSECURITY_ATTRIBUTES = *mut core::ffi::c_void;
pub type LPPROC_THREAD_ATTRIBUTE_LIST = *mut core::ffi::c_void;
pub type LPSTARTUPINFOEXW = *mut STARTUPINFOEXW;
pub type LPPROCESS_INFORMATION = *mut PROCESS_INFORMATION;
pub type SECURITY_INFORMATION = u32;
pub type SE_OBJECT_TYPE = u32;
pub type ACL_PTR = *mut core::ffi::c_void;
pub type ATTRIBUTE_TARGET = *mut core::ffi::c_void;
pub type ACCESS_MASK = u32;
pub type ACCESS_MODE = u32;
pub type TRUSTEE_FORM = u32;
pub type TRUSTEE_TYPE = u32;
pub type MULTIPLE_TRUSTEE_OPERATION = u32;

// ---------------------------------------------------------------------------
// Constants — values match windows-sys / Win32 headers (advapi32, kernel32).
// ---------------------------------------------------------------------------

/// `CreateProcessW` `dwCreationFlags` — interpret `lpStartupInfo` as
/// `STARTUPINFOEXW` carrying a `PROC_THREAD_ATTRIBUTE_LIST`.
pub const EXTENDED_STARTUPINFO_PRESENT: DWORD = 0x0008_0000;

/// `UpdateProcThreadAttribute` `Attribute` — attach `SECURITY_CAPABILITIES`
/// (which carries the AppContainer SID + capability list) to the child.
pub const PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES: usize = 0x0002_0009;

/// `WaitForSingleObject` timeout — wait forever.
pub const INFINITE: DWORD = 0xFFFF_FFFF;

/// `WaitForSingleObject` success return.
pub const WAIT_OBJECT_0: DWORD = 0x0000_0000;

/// `SetEntriesInAclW` success return.
pub const ERROR_SUCCESS: DWORD = 0;

/// `GetNamedSecurityInfoW` / `SetNamedSecurityInfoW` `ObjectType` — file
/// system object.
pub const SE_FILE_OBJECT: SE_OBJECT_TYPE = 1;

/// `SECURITY_INFORMATION` — DACL is being set.
pub const DACL_SECURITY_INFORMATION: SECURITY_INFORMATION = 0x0000_0004;

/// `EXPLICIT_ACCESS_W` `grfAccessMode` — grant access.
pub const GRANT_ACCESS: ACCESS_MODE = 1;

/// `EXPLICIT_ACCESS_W` `grfInheritance` — no inheritance.
pub const NO_INHERITANCE: DWORD = 0;

/// `EXPLICIT_ACCESS_W` `grfInheritance` — propagate to subobjects + containers.
pub const SUB_CONTAINERS_AND_OBJECTS_INHERIT: DWORD = 0x0000_0003;

/// `TRUSTEE_W` `MultipleTrusteeOperation` — no multiple trustee.
pub const NO_MULTIPLE_TRUSTEE: MULTIPLE_TRUSTEE_OPERATION = 0;

/// `TRUSTEE_W` `TrusteeForm` — `ptstrName` is a SID pointer.
pub const TRUSTEE_IS_SID: TRUSTEE_FORM = 0;

/// `TRUSTEE_W` `TrusteeType` — well-known group (used for AppContainer SID).
pub const TRUSTEE_IS_WELL_KNOWN_GROUP: TRUSTEE_TYPE = 5;

/// `EXPLICIT_ACCESS_W` `grfAccessPermissions` — generic file write/read/execute.
pub const FILE_GENERIC_READ: ACCESS_MASK = 0x0012_0089;
pub const FILE_GENERIC_WRITE: ACCESS_MASK = 0x0012_0116;
pub const FILE_GENERIC_EXECUTE: ACCESS_MASK = 0x0012_00A0;
pub const FILE_ALL_ACCESS: ACCESS_MASK = 0x001F_01FF;

// ---------------------------------------------------------------------------
// Structs — `#[repr(C)]` layouts must match the Win32 headers exactly. Field
// names mirror windows-sys / MSDN.
// ---------------------------------------------------------------------------

/// `_STARTUPINFOW` from <processthreadsapi.h>.
#[repr(C)]
pub struct STARTUPINFOW {
    pub cb: DWORD,
    pub lpReserved: LPWSTR,
    pub lpDesktop: LPWSTR,
    pub lpTitle: LPWSTR,
    pub dwX: DWORD,
    pub dwY: DWORD,
    pub dwXSize: DWORD,
    pub dwYSize: DWORD,
    pub dwXCountChars: DWORD,
    pub dwYCountChars: DWORD,
    pub dwFillAttribute: DWORD,
    pub dwFlags: DWORD,
    pub wShowWindow: WORD,
    pub cbReserved2: WORD,
    pub lpReserved2: LPBYTE,
    pub hStdInput: HANDLE,
    pub hStdOutput: HANDLE,
    pub hStdError: HANDLE,
}

/// `_STARTUPINFOEXW` — extends `STARTUPINFOW` with an attribute list.
#[repr(C)]
pub struct STARTUPINFOEXW {
    pub StartupInfo: STARTUPINFOW,
    pub lpAttributeList: LPPROC_THREAD_ATTRIBUTE_LIST,
}

/// `_PROCESS_INFORMATION` — populated by a successful `CreateProcessW`.
#[repr(C)]
pub struct PROCESS_INFORMATION {
    pub hProcess: HANDLE,
    pub hThread: HANDLE,
    pub dwProcessId: DWORD,
    pub dwThreadId: DWORD,
}

/// `_SID_AND_ATTRIBUTES` — used in capability arrays attached to
/// `SECURITY_CAPABILITIES`.
#[repr(C)]
pub struct SID_AND_ATTRIBUTES {
    pub Sid: PSID,
    pub Attributes: DWORD,
}

/// `_SECURITY_CAPABILITIES` — the payload of
/// `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES`.
#[repr(C)]
pub struct SECURITY_CAPABILITIES {
    pub AppContainerSid: PSID,
    pub Capabilities: *mut SID_AND_ATTRIBUTES,
    pub CapabilityCount: DWORD,
    pub Reserved: DWORD,
}

/// `_TRUSTEE_W` — identifies the principal in an `EXPLICIT_ACCESS_W` entry.
#[repr(C)]
pub struct TRUSTEE_W {
    pub pMultipleTrustee: *mut Self,
    pub MultipleTrusteeOperation: MULTIPLE_TRUSTEE_OPERATION,
    pub TrusteeForm: TRUSTEE_FORM,
    pub TrusteeType: TRUSTEE_TYPE,
    /// Despite the `LPWSTR`-shaped name in Win32 headers, when `TrusteeForm`
    /// is `TRUSTEE_IS_SID` this carries a `PSID`. Cast at the call site.
    pub ptstrName: LPWSTR,
}

/// `_EXPLICIT_ACCESS_W` — one ACE-to-be passed to `SetEntriesInAclW`.
#[repr(C)]
pub struct EXPLICIT_ACCESS_W {
    pub grfAccessPermissions: ACCESS_MASK,
    pub grfAccessMode: ACCESS_MODE,
    pub grfInheritance: DWORD,
    pub Trustee: TRUSTEE_W,
}

// ---------------------------------------------------------------------------
// AppContainer profile lifecycle
// ---------------------------------------------------------------------------

#[link(name = "userenv")]
unsafe extern "system" {
    /// Create a new AppContainer profile keyed by a Unicode name. The
    /// returned `PSID` is the AppContainer SID we grant per-path ACLs to.
    pub fn CreateAppContainerProfile(
        pszAppContainerName: LPCWSTR,
        pszDisplayName: LPCWSTR,
        pszDescription: LPCWSTR,
        pCapabilities: PVOID,
        dwCapabilityCount: DWORD,
        ppSidAppContainerSid: *mut PSID,
    ) -> i32;

    /// Remove an AppContainer profile previously created with
    /// `CreateAppContainerProfile`. Safe to call after a successful spawn.
    pub fn DeleteAppContainerProfile(pszAppContainerName: LPCWSTR) -> i32;

    /// Look up an existing AppContainer SID by profile name (cheaper than
    /// recreating the profile on every invocation).
    pub fn DeriveAppContainerSidFromAppContainerName(
        pszAppContainerName: LPCWSTR,
        ppsidAppContainerSid: *mut PSID,
    ) -> i32;
}

// ---------------------------------------------------------------------------
// Per-path ACL grants
// ---------------------------------------------------------------------------

#[link(name = "advapi32")]
unsafe extern "system" {
    /// Read the current security descriptor for a named object (file path)
    /// so we can extend its DACL with an ACE granting the AppContainer SID
    /// write access.
    pub fn GetNamedSecurityInfoW(
        pObjectName: LPCWSTR,
        ObjectType: SE_OBJECT_TYPE,
        SecurityInfo: SECURITY_INFORMATION,
        ppsidOwner: *mut PSID,
        ppsidGroup: *mut PSID,
        ppDacl: *mut ACL_PTR,
        ppSacl: *mut ACL_PTR,
        ppSecurityDescriptor: *mut PSECURITY_DESCRIPTOR,
    ) -> DWORD;

    /// Write the updated DACL back. The companion to `GetNamedSecurityInfoW`
    /// after extending the ACL via `SetEntriesInAclW`.
    pub fn SetNamedSecurityInfoW(
        pObjectName: LPWSTR,
        ObjectType: SE_OBJECT_TYPE,
        SecurityInfo: SECURITY_INFORMATION,
        psidOwner: PSID,
        psidGroup: PSID,
        pDacl: ACL_PTR,
        pSacl: ACL_PTR,
    ) -> DWORD;

    /// Build a new DACL by merging an explicit ACE (granting our
    /// AppContainer SID) into an existing one. Returns the new DACL via
    /// `NewAcl`; the caller owns it and must `LocalFree` it.
    pub fn SetEntriesInAclW(
        cCountOfExplicitEntries: u32,
        pListOfExplicitEntries: PVOID,
        OldAcl: ACL_PTR,
        NewAcl: *mut ACL_PTR,
    ) -> DWORD;

    /// Convert a SID into the textual S-1-... form. Useful for audit-log
    /// entries identifying which AppContainer ran a particular rotation.
    pub fn ConvertSidToStringSidW(Sid: PSID, StringSid: *mut LPWSTR) -> BOOL;
}

// ---------------------------------------------------------------------------
// Process creation with security capabilities
// ---------------------------------------------------------------------------

#[link(name = "kernel32")]
unsafe extern "system" {
    /// Allocate space for a `PROC_THREAD_ATTRIBUTE_LIST`. Used to attach the
    /// `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES` attribute (which
    /// carries the AppContainer SID) onto the spawn.
    pub fn InitializeProcThreadAttributeList(
        lpAttributeList: LPPROC_THREAD_ATTRIBUTE_LIST,
        dwAttributeCount: DWORD,
        dwFlags: DWORD,
        lpSize: *mut usize,
    ) -> BOOL;

    /// Set the `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES` attribute on
    /// the list. The value is a `SECURITY_CAPABILITIES` struct pointing at
    /// our AppContainer SID plus an empty capability array.
    pub fn UpdateProcThreadAttribute(
        lpAttributeList: LPPROC_THREAD_ATTRIBUTE_LIST,
        dwFlags: DWORD,
        Attribute: usize,
        lpValue: PVOID,
        cbSize: usize,
        lpPreviousValue: PVOID,
        lpReturnSize: *mut usize,
    ) -> BOOL;

    /// Release the attribute list once `CreateProcessW` returns.
    pub fn DeleteProcThreadAttributeList(lpAttributeList: LPPROC_THREAD_ATTRIBUTE_LIST);

    /// Spawn the child. We pass `EXTENDED_STARTUPINFO_PRESENT` in
    /// `dwCreationFlags` and `lpStartupInfo` points at a `STARTUPINFOEXW`
    /// whose `lpAttributeList` carries the security capabilities.
    pub fn CreateProcessW(
        lpApplicationName: LPCWSTR,
        lpCommandLine: LPWSTR,
        lpProcessAttributes: LPSECURITY_ATTRIBUTES,
        lpThreadAttributes: LPSECURITY_ATTRIBUTES,
        bInheritHandles: BOOL,
        dwCreationFlags: DWORD,
        lpEnvironment: PVOID,
        lpCurrentDirectory: LPCWSTR,
        lpStartupInfo: LPSTARTUPINFOEXW,
        lpProcessInformation: LPPROCESS_INFORMATION,
    ) -> BOOL;

    /// Wait for the spawned child to exit before tearing down the
    /// AppContainer profile.
    pub fn WaitForSingleObject(hHandle: HANDLE, dwMilliseconds: DWORD) -> DWORD;

    /// Read the child's exit code so the launcher can propagate it.
    pub fn GetExitCodeProcess(hProcess: HANDLE, lpExitCode: *mut DWORD) -> BOOL;

    /// Close a handle returned by `CreateProcessW`.
    pub fn CloseHandle(hObject: HANDLE) -> BOOL;

    /// Release memory allocated by `SetEntriesInAclW` /
    /// `ConvertSidToStringSidW` etc.
    pub fn LocalFree(hMem: HANDLE) -> HANDLE;
}
