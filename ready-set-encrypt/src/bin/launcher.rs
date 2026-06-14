//! `ready-set-encrypt-launcher` — Windows AppContainer wrapper.
//!
//! Invoked by `sandbox::wrap()` on Windows. The plugin prefixes the user's
//! argv with `[launcher.exe, --project-root <p>, --tmpdir <t>, --cache <c>,
//! --extra-write <e>..., --container-name <name>, --, <child argv>]`.
//!
//! The launcher:
//! 1. Creates (or reuses) an AppContainer profile keyed by `--container-name`,
//!    retrieving its SID.
//! 2. For each writable path (`project-root`, `tmpdir`, `cache`, each
//!    `extra-write`), adds an ACE granting the AppContainer SID
//!    `FILE_ALL_ACCESS` with `SUB_CONTAINERS_AND_OBJECTS_INHERIT`.
//! 3. Allocates a `PROC_THREAD_ATTRIBUTE_LIST`, attaches a
//!    `SECURITY_CAPABILITIES` carrying just the AppContainer SID (no
//!    capabilities — pure write-allowlist isolation).
//! 4. Spawns the child via `CreateProcessW` with
//!    `EXTENDED_STARTUPINFO_PRESENT`, waits for it, propagates the exit
//!    code.
//! 5. Tears down: cleans the attribute list, closes handles, leaves the
//!    AppContainer profile in place (cheap to reuse next invocation; the
//!    SID is derived deterministically from `--container-name`).
//!
//! Threat model + rationale: see the `sandbox.rs` Windows backend docs.

// This binary is Windows-only. On other targets it compiles to a stub
// `main` that errors out; the `Cargo.toml` `[[bin]]` entry still builds
// universally to keep `cargo check --workspace` working everywhere.
#![cfg_attr(target_os = "windows", allow(unsafe_code))]
// Doc comments reference Win32 type / API names freely; same rationale
// as `win_ffi.rs`. The remaining allows quiet pedantic lints that fire
// on legitimate Win32 FFI patterns (raw-pointer borrows, in/out struct
// pointers, etc.) — fixing them would obscure the wire-level call
// shape that matters for FFI review.
#![allow(clippy::doc_markdown)]
#![cfg_attr(
    target_os = "windows",
    allow(
        clippy::borrow_as_ptr,
        clippy::ref_as_ptr,
        clippy::ptr_as_ptr,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        clippy::missing_errors_doc,
        clippy::missing_panics_doc,
        clippy::missing_safety_doc,
        clippy::too_many_lines,
    )
)]

#[cfg(not(target_os = "windows"))]
fn main() -> std::process::ExitCode {
    eprintln!(
        "ready-set-encrypt-launcher: Windows-only binary; should never run on \
         this platform. The sandbox::wrap dispatch on macOS/Linux uses \
         sandbox-exec/bwrap and never invokes the launcher."
    );
    std::process::ExitCode::from(2)
}

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    match windows_main::run() {
        Ok(code) => std::process::ExitCode::from(u8::try_from(code & 0xFF).unwrap_or(0xFF)),
        Err(err) => {
            eprintln!("ready-set-encrypt-launcher: {err}");
            std::process::ExitCode::from(2)
        },
    }
}

#[cfg(target_os = "windows")]
mod windows_main {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;
    use std::ptr;

    use ready_set_encrypt::win_ffi::{
        BOOL, CloseHandle, ConvertSidToStringSidW, CreateAppContainerProfile, CreateProcessW,
        DACL_SECURITY_INFORMATION, DWORD, DeleteProcThreadAttributeList,
        DeriveAppContainerSidFromAppContainerName, EXPLICIT_ACCESS_W, EXTENDED_STARTUPINFO_PRESENT,
        FILE_ALL_ACCESS, GRANT_ACCESS, GetExitCodeProcess, GetNamedSecurityInfoW, HANDLE, INFINITE,
        InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST, LPSECURITY_ATTRIBUTES,
        LPSTARTUPINFOEXW, LPWSTR, LocalFree, NO_INHERITANCE, NO_MULTIPLE_TRUSTEE,
        PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, PROCESS_INFORMATION, PSID, SE_FILE_OBJECT,
        SECURITY_CAPABILITIES, STARTUPINFOEXW, STARTUPINFOW, SUB_CONTAINERS_AND_OBJECTS_INHERIT,
        SetEntriesInAclW, SetNamedSecurityInfoW, TRUSTEE_IS_SID, TRUSTEE_IS_WELL_KNOWN_GROUP,
        TRUSTEE_W, UpdateProcThreadAttribute, WAIT_OBJECT_0, WaitForSingleObject,
    };

    /// Parsed launcher CLI.
    #[derive(Debug)]
    struct Args {
        project_root: PathBuf,
        tmpdir: PathBuf,
        cache: PathBuf,
        extra_writes: Vec<PathBuf>,
        container_name: String,
        /// Everything after the final `--`. argv[0] is the executable.
        child_argv: Vec<OsString>,
    }

    pub fn run() -> Result<DWORD, String> {
        let args = parse_args()?;
        validate_container_name(&args.container_name)?;
        let sid = ensure_appcontainer_sid(&args.container_name)?;
        let _sid_guard = SidGuard(sid);

        // Grant write access for each writable path. Failures are
        // surfaced (rather than silently degraded) — a sandbox that
        // doesn't grant what we promised would mislead the audit log.
        for path in std::iter::once(&args.project_root)
            .chain(std::iter::once(&args.tmpdir))
            .chain(std::iter::once(&args.cache))
            .chain(args.extra_writes.iter())
        {
            grant_appcontainer_write(path, sid)?;
        }

        let exit_code = spawn_child_in_appcontainer(sid, &args.child_argv)?;
        Ok(exit_code)
    }

    // -------------------------------------------------------------------
    // Arg parsing
    // -------------------------------------------------------------------

    fn parse_args() -> Result<Args, String> {
        let argv: Vec<OsString> = std::env::args_os().collect();
        let mut project_root: Option<PathBuf> = None;
        let mut tmpdir: Option<PathBuf> = None;
        let mut cache: Option<PathBuf> = None;
        let mut extra_writes: Vec<PathBuf> = Vec::new();
        let mut container_name: Option<String> = None;
        let mut child_argv: Vec<OsString> = Vec::new();
        let mut i = 1;
        while i < argv.len() {
            let arg = &argv[i];
            if arg == "--" {
                child_argv = argv[i + 1..].to_vec();
                break;
            }
            let key = arg.to_str().ok_or("non-UTF8 argv")?;
            let value = argv
                .get(i + 1)
                .ok_or_else(|| format!("flag `{key}` missing value"))?;
            match key {
                "--project-root" => project_root = Some(value.into()),
                "--tmpdir" => tmpdir = Some(value.into()),
                "--cache" => cache = Some(value.into()),
                "--extra-write" => extra_writes.push(value.into()),
                "--container-name" => container_name = Some(value.to_string_lossy().into_owned()),
                other => return Err(format!("unknown flag `{other}`")),
            }
            i += 2;
        }
        Ok(Args {
            project_root: project_root.ok_or("--project-root is required")?,
            tmpdir: tmpdir.ok_or("--tmpdir is required")?,
            cache: cache.ok_or("--cache is required")?,
            extra_writes,
            container_name: container_name.ok_or("--container-name is required")?,
            child_argv,
        })
    }

    // -------------------------------------------------------------------
    // AppContainer SID lifecycle
    // -------------------------------------------------------------------

    /// Validate the AppContainer profile name against a conservative
    /// charset before handing it to Win32. The sandbox crate generates
    /// names like `ready-set-encrypt.<32-hex>`, so we only need to
    /// allow ASCII alphanumeric, `.`, `-`, `_`. Rejecting anything else
    /// defends against passing maliciously-crafted UTF-16 sequences,
    /// path-traversal-style names, or strings the Win32 namespace
    /// would interpret unexpectedly.
    fn validate_container_name(name: &str) -> Result<(), String> {
        if name.is_empty() {
            return Err("--container-name must not be empty".into());
        }
        if name.len() > 64 {
            return Err(format!(
                "--container-name `{name}` is {} chars; max 64",
                name.len()
            ));
        }
        for c in name.chars() {
            if !(c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_') {
                return Err(format!(
                    "--container-name `{name}` contains disallowed char `{c}`; \
                     allowed: ASCII alphanumeric, `.`, `-`, `_`"
                ));
            }
        }
        Ok(())
    }

    fn ensure_appcontainer_sid(name: &str) -> Result<PSID, String> {
        let wide_name = to_wide_null(name);
        let mut sid: PSID = ptr::null_mut();

        // Try the create path first. If the profile already exists
        // (`HRESULT_FROM_WIN32(ERROR_ALREADY_EXISTS)`), derive instead.
        // We don't distinguish the specific HRESULT — any failure of
        // create falls through to derive.
        let display = to_wide_null(name);
        let description = to_wide_null("ready-set-encrypt rotation sandbox");
        let hr = unsafe {
            CreateAppContainerProfile(
                wide_name.as_ptr(),
                display.as_ptr(),
                description.as_ptr(),
                ptr::null_mut(),
                0,
                &mut sid,
            )
        };
        if hr == 0 {
            return Ok(sid);
        }
        let hr2 =
            unsafe { DeriveAppContainerSidFromAppContainerName(wide_name.as_ptr(), &mut sid) };
        if hr2 != 0 {
            return Err(format!(
                "CreateAppContainerProfile failed (HRESULT 0x{hr:08x}) and \
                 DeriveAppContainerSidFromAppContainerName fallback failed \
                 (HRESULT 0x{hr2:08x})"
            ));
        }
        Ok(sid)
    }

    /// RAII for the AppContainer SID. We don't delete the profile because
    /// the next rotation invocation will reuse the deterministic name; we
    /// only need to free the in-process SID allocation.
    struct SidGuard(PSID);

    impl Drop for SidGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // LocalFree is correct for SIDs returned by both
                // CreateAppContainerProfile and
                // DeriveAppContainerSidFromAppContainerName per MSDN.
                let _ = unsafe { LocalFree(self.0) };
            }
        }
    }

    // -------------------------------------------------------------------
    // Per-path ACL grants
    // -------------------------------------------------------------------

    fn grant_appcontainer_write(path: &PathBuf, sid: PSID) -> Result<(), String> {
        let wide_path = to_wide_null(&path.display().to_string());

        let mut old_dacl: *mut core::ffi::c_void = ptr::null_mut();
        let mut security_descriptor: *mut core::ffi::c_void = ptr::null_mut();
        let rc = unsafe {
            GetNamedSecurityInfoW(
                wide_path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut old_dacl,
                ptr::null_mut(),
                &mut security_descriptor,
            )
        };
        if rc != 0 {
            return Err(format!(
                "GetNamedSecurityInfoW({}) failed: {rc}",
                path.display()
            ));
        }
        let _sd_guard = LocalFreeGuard(security_descriptor as HANDLE);

        let mut explicit = EXPLICIT_ACCESS_W {
            grfAccessPermissions: FILE_ALL_ACCESS,
            grfAccessMode: GRANT_ACCESS,
            grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
            Trustee: TRUSTEE_W {
                pMultipleTrustee: ptr::null_mut(),
                MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_WELL_KNOWN_GROUP,
                ptstrName: sid as LPWSTR,
            },
        };
        let mut new_dacl: *mut core::ffi::c_void = ptr::null_mut();
        let set_rc = unsafe {
            SetEntriesInAclW(
                1,
                &mut explicit as *mut _ as *mut core::ffi::c_void,
                old_dacl,
                &mut new_dacl,
            )
        };
        if set_rc != 0 {
            return Err(format!("SetEntriesInAclW failed: {set_rc}"));
        }
        let _new_dacl_guard = LocalFreeGuard(new_dacl as HANDLE);

        let apply_rc = unsafe {
            SetNamedSecurityInfoW(
                wide_path.as_ptr().cast_mut(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                new_dacl,
                ptr::null_mut(),
            )
        };
        if apply_rc != 0 {
            return Err(format!(
                "SetNamedSecurityInfoW({}) failed: {apply_rc}",
                path.display()
            ));
        }
        Ok(())
    }

    /// RAII over `LocalFree` for buffers returned by Win32 ACL APIs.
    struct LocalFreeGuard(HANDLE);

    impl Drop for LocalFreeGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                let _ = unsafe { LocalFree(self.0) };
            }
        }
    }

    // -------------------------------------------------------------------
    // Child process spawn with SECURITY_CAPABILITIES
    // -------------------------------------------------------------------

    fn spawn_child_in_appcontainer(sid: PSID, child_argv: &[OsString]) -> Result<DWORD, String> {
        if child_argv.is_empty() {
            return Err("no child argv given (use `-- <command> [args...]`)".into());
        }

        // Step 1: size the PROC_THREAD_ATTRIBUTE_LIST.
        let mut size: usize = 0;
        unsafe {
            InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut size);
        }
        let mut attr_buf: Vec<u8> = vec![0u8; size];
        let attr_list: LPPROC_THREAD_ATTRIBUTE_LIST = attr_buf.as_mut_ptr().cast();
        let init_ok = unsafe { InitializeProcThreadAttributeList(attr_list, 1, 0, &mut size) };
        if init_ok == 0 {
            return Err("InitializeProcThreadAttributeList failed".into());
        }
        // Guard ensures DeleteProcThreadAttributeList runs even on early returns.
        let _attr_guard = AttrListGuard(attr_list);

        // Step 2: build SECURITY_CAPABILITIES with just the AppContainer SID.
        let mut caps = SECURITY_CAPABILITIES {
            AppContainerSid: sid,
            Capabilities: ptr::null_mut(),
            CapabilityCount: 0,
            Reserved: 0,
        };
        let upd_ok = unsafe {
            UpdateProcThreadAttribute(
                attr_list,
                0,
                PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
                &mut caps as *mut _ as *mut core::ffi::c_void,
                core::mem::size_of::<SECURITY_CAPABILITIES>(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if upd_ok == 0 {
            return Err("UpdateProcThreadAttribute(SECURITY_CAPABILITIES) failed".into());
        }

        // Step 3: pack STARTUPINFOEXW + command line.
        let mut startup = STARTUPINFOEXW {
            StartupInfo: zeroed_startupinfo(),
            lpAttributeList: attr_list,
        };
        startup.StartupInfo.cb = core::mem::size_of::<STARTUPINFOEXW>() as DWORD;
        let mut cmdline = build_command_line(child_argv);
        let app_name = to_wide_null(child_argv[0].to_string_lossy().as_ref());

        let mut proc_info = PROCESS_INFORMATION {
            hProcess: ptr::null_mut(),
            hThread: ptr::null_mut(),
            dwProcessId: 0,
            dwThreadId: 0,
        };

        let spawned = unsafe {
            CreateProcessW(
                app_name.as_ptr(),
                cmdline.as_mut_ptr(),
                ptr::null_mut() as LPSECURITY_ATTRIBUTES,
                ptr::null_mut() as LPSECURITY_ATTRIBUTES,
                0, // bInheritHandles
                EXTENDED_STARTUPINFO_PRESENT,
                ptr::null_mut(),
                ptr::null(),
                &mut startup as LPSTARTUPINFOEXW,
                &mut proc_info as *mut PROCESS_INFORMATION,
            )
        };
        if spawned == 0 {
            return Err(format!(
                "CreateProcessW failed for `{}`",
                child_argv[0].to_string_lossy()
            ));
        }

        // Step 4: wait, fetch exit code, close handles.
        let wait_rc = unsafe { WaitForSingleObject(proc_info.hProcess, INFINITE) };
        if wait_rc != WAIT_OBJECT_0 {
            unsafe {
                CloseHandle(proc_info.hThread);
                CloseHandle(proc_info.hProcess);
            }
            return Err(format!("WaitForSingleObject returned 0x{wait_rc:08x}"));
        }
        let mut exit_code: DWORD = 0;
        let got_exit = unsafe { GetExitCodeProcess(proc_info.hProcess, &mut exit_code) };
        unsafe {
            CloseHandle(proc_info.hThread);
            CloseHandle(proc_info.hProcess);
        }
        if got_exit == 0 {
            return Err("GetExitCodeProcess failed".into());
        }
        Ok(exit_code)
    }

    struct AttrListGuard(LPPROC_THREAD_ATTRIBUTE_LIST);

    impl Drop for AttrListGuard {
        fn drop(&mut self) {
            unsafe {
                DeleteProcThreadAttributeList(self.0);
            }
        }
    }

    fn zeroed_startupinfo() -> STARTUPINFOW {
        // SAFETY: STARTUPINFOW is a POD struct (#[repr(C)], no Rust
        // invariants); all-zero is a valid representation that means
        // "use defaults".
        unsafe { core::mem::zeroed() }
    }

    // -------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------

    /// Convert a Rust string to a null-terminated wide string (UTF-16).
    fn to_wide_null(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// Build a single `lpCommandLine` from child argv using Win32 escaping
    /// rules. The first element is the program name; subsequent elements
    /// are space-separated and individually quoted if they contain spaces
    /// or quotes.
    ///
    /// Implements the algorithm documented at
    /// <https://learn.microsoft.com/en-us/cpp/cpp/main-function-command-line-args#parsing-c-command-line-arguments>:
    /// for each `"` inside a quoted segment, emit `2*N + 1` backslashes
    /// (where N is the count of immediately-preceding backslashes), then
    /// the `"`. At the end of the segment, emit `2*N` backslashes before
    /// the closing `"`. This prevents the classic flaw where `arg\"`
    /// would otherwise be parsed by the child as `arg` followed by an
    /// unquoted-quote that breaks out of the string and lets an attacker
    /// smuggle extra args.
    fn build_command_line(argv: &[OsString]) -> Vec<u16> {
        let mut joined = String::new();
        for (i, a) in argv.iter().enumerate() {
            if i > 0 {
                joined.push(' ');
            }
            let s = a.to_string_lossy();
            if needs_quoting(&s) {
                joined.push('"');
                let mut backslashes = 0usize;
                for c in s.chars() {
                    match c {
                        '\\' => {
                            backslashes += 1;
                        },
                        '"' => {
                            // 2N+1 backslashes, then the quote.
                            for _ in 0..(2 * backslashes + 1) {
                                joined.push('\\');
                            }
                            joined.push('"');
                            backslashes = 0;
                        },
                        other => {
                            // Pending backslashes are literal here.
                            for _ in 0..backslashes {
                                joined.push('\\');
                            }
                            joined.push(other);
                            backslashes = 0;
                        },
                    }
                }
                // Closing quote: any pending backslashes must be doubled
                // so the parser sees them as literal, not as escapes for
                // the closing quote.
                for _ in 0..(2 * backslashes) {
                    joined.push('\\');
                }
                joined.push('"');
            } else {
                joined.push_str(&s);
            }
        }
        let mut wide: Vec<u16> = joined.encode_utf16().collect();
        wide.push(0);
        wide
    }

    fn needs_quoting(s: &str) -> bool {
        s.is_empty() || s.chars().any(|c| c.is_whitespace() || c == '"')
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn cmdline(argv: &[&str]) -> String {
            let v: Vec<OsString> = argv.iter().map(|s| s.to_string().into()).collect();
            let wide = build_command_line(&v);
            // Drop the null terminator before decoding.
            String::from_utf16(&wide[..wide.len() - 1]).unwrap()
        }

        #[test]
        fn no_quoting_when_arg_is_plain() {
            assert_eq!(cmdline(&["foo.exe", "bar"]), "foo.exe bar");
        }

        #[test]
        fn quotes_args_with_spaces() {
            assert_eq!(cmdline(&["foo.exe", "a b"]), "foo.exe \"a b\"");
        }

        #[test]
        fn escapes_internal_quote_with_backslash() {
            assert_eq!(cmdline(&["x", "a\"b"]), "x \"a\\\"b\"");
        }

        #[test]
        fn doubles_backslashes_before_closing_quote() {
            // The arg `a\` inside a quoted segment: trailing `\` would
            // otherwise escape the closing quote. Must emit `a\\"`.
            assert_eq!(cmdline(&["x", "a\\"]), "x \"a\\\\\"");
        }

        #[test]
        fn handles_backslash_before_internal_quote() {
            // arg `a\"b`: the `\` before `"` would normally escape the
            // quote. Must emit `\\\` then `"`: 2*1+1 = 3 backslashes.
            assert_eq!(cmdline(&["x", "a\\\"b"]), "x \"a\\\\\\\"b\"");
        }

        #[test]
        fn handles_multiple_backslashes_before_quote() {
            // arg `a\\\"b`: 3 backslashes before quote → 2*3+1 = 7
            // backslashes then `"`.
            assert_eq!(cmdline(&["x", "a\\\\\\\"b"]), "x \"a\\\\\\\\\\\\\\\"b\"");
        }

        #[test]
        fn container_name_accepts_safe_chars() {
            assert!(validate_container_name("ready-set-encrypt.abc123_def").is_ok());
            assert!(validate_container_name("a").is_ok());
        }

        #[test]
        fn container_name_rejects_empty() {
            assert!(validate_container_name("").is_err());
        }

        #[test]
        fn container_name_rejects_oversized() {
            let huge = "x".repeat(65);
            assert!(validate_container_name(&huge).is_err());
        }

        #[test]
        fn container_name_rejects_path_traversal() {
            assert!(validate_container_name("../escape").is_err());
            assert!(validate_container_name("a/b").is_err());
            assert!(validate_container_name("a\\b").is_err());
        }

        #[test]
        fn container_name_rejects_unicode() {
            assert!(validate_container_name("café").is_err());
            assert!(validate_container_name("name\u{0000}").is_err());
        }
    }
}
