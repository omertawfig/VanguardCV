//! Cross-platform OS integration helpers (external browser launch and native
//! Windows file open/save dialogs).
//!
//! ## Why this module exists
//!
//! 1. **External URLs on Windows 11 (`#![windows_subsystem = "windows"]`)**:
//!    `open` v5.4+ defaults to spawning `powershell.exe -NoProfile -Command ...`
//!    unless `shellexecute-on-windows` + `open::that_detached` is used. Inside a
//!    release GUI binary with detached console handles, spawning `powershell.exe`
//!    adds multi-second latency and can be blocked or fail under Windows 11
//!    security policies. Calling Win32 `ShellExecuteW` directly on a COM
//!    STA-initialized thread opens the user's default browser immediately with
//!    zero console subprocesses and safely handles OAuth URLs containing `&`.
//! 2. **Native file open/save dialogs on Windows 11**:
//!    GPUI 0.2.2's `prompt_for_paths` / `prompt_for_new_path` runs
//!    `IFileOpenDialog::Show()` synchronously on the main DirectX UI thread
//!    with `CLSCTX_ALL`, overwrites default `FOS_*` options without
//!    `FOS_FORCEFILESYSTEM | FOS_NOCHANGEDIR`, passes the prompt string to
//!    `SetOkButtonLabel` instead of `SetTitle`, and silently discards any
//!    `Show()` `HRESULT` failure as `Ok(None)` (user cancelled). Running the
//!    dialog on a dedicated STA thread with proper `FOS_*` flags, file type
//!    filters, real cancellation detection (`ERROR_CANCELLED`), and an automatic
//!    fallback to classic Win32 `GetOpenFileNameW` / `GetSaveFileNameW`
//!    (`comdlg32.dll`) guarantees popups open reliably across Windows 10/11.

use std::path::PathBuf;

use anyhow::{Result, anyhow, bail};

/// File type filter shown in native Open/Save file dialogs.
#[derive(Debug, Clone)]
pub struct FileDialogFilter {
    pub label: &'static str,
    pub extensions: &'static [&'static str],
}

/// Request parameters for a native single-file Open dialog.
#[derive(Debug, Clone)]
pub struct OpenFileRequest {
    pub title: String,
    pub initial_dir: Option<PathBuf>,
    pub filters: Vec<FileDialogFilter>,
}

/// Request parameters for a native Save File dialog.
#[derive(Debug, Clone)]
pub struct SaveFileRequest {
    pub title: String,
    pub initial_dir: Option<PathBuf>,
    pub default_name: Option<String>,
    pub default_extension: Option<String>,
    pub filters: Vec<FileDialogFilter>,
}

/// Opens an external `http://` or `https://` URL in the user's default browser.
///
/// On Windows, calls Win32 `ShellExecuteW` directly (zero `powershell.exe` or
/// `cmd.exe` subprocesses) with COM initialized in STA mode, falling back to
/// `open::that_detached` if needed.
pub fn open_external_url(url: &str) -> Result<()> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        bail!("Cannot open an empty URL.");
    }
    if trimmed.chars().any(|c| c.is_control()) {
        bail!("Refusing to open URL containing control characters.");
    }
    let parsed = url::Url::parse(trimmed).map_err(|_| anyhow!("Refusing to open invalid URL."))?;
    if !matches!(parsed.scheme(), "https" | "http") || parsed.host_str().is_none() {
        bail!("Refusing to open non-HTTP(S) URL.");
    }

    #[cfg(target_os = "windows")]
    {
        windows_impl::open_url(trimmed)
    }

    #[cfg(not(target_os = "windows"))]
    {
        open::that_detached(trimmed)
            .or_else(|_| open::that(trimmed))
            .map_err(|err| anyhow!("Could not open system browser: {err}"))
    }
}

/// Opens a native Windows file-open dialog on a dedicated STA thread so it
/// never deadlocks GPUI's DirectX thread, with automatic `GetOpenFileNameW`
/// fallback if COM `IFileOpenDialog` fails.
#[cfg(target_os = "windows")]
pub fn pick_open_file_windows(
    request: OpenFileRequest,
) -> tokio::sync::oneshot::Receiver<Result<Option<PathBuf>>> {
    let owner_hwnd = windows_impl::current_process_owner_hwnd();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let spawn_res = std::thread::Builder::new()
        .name("vanguard-open-dialog".to_string())
        .spawn(move || {
            let outcome = windows_impl::run_open_dialog(&request, owner_hwnd);
            let _ = tx.send(outcome);
        });
    if let Err(err) = spawn_res {
        let (err_tx, err_rx) = tokio::sync::oneshot::channel();
        let _ = err_tx.send(Err(anyhow!("Could not spawn file dialog thread: {err}")));
        return err_rx;
    }
    rx
}

/// Opens a native Windows file-save dialog on a dedicated STA thread so it
/// never deadlocks GPUI's DirectX thread, with automatic `GetSaveFileNameW`
/// fallback if COM `IFileSaveDialog` fails.
#[cfg(target_os = "windows")]
pub fn pick_save_file_windows(
    request: SaveFileRequest,
) -> tokio::sync::oneshot::Receiver<Result<Option<PathBuf>>> {
    let owner_hwnd = windows_impl::current_process_owner_hwnd();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let spawn_res = std::thread::Builder::new()
        .name("vanguard-save-dialog".to_string())
        .spawn(move || {
            let outcome = windows_impl::run_save_dialog(&request, owner_hwnd);
            let _ = tx.send(outcome);
        });
    if let Err(err) = spawn_res {
        let (err_tx, err_rx) = tokio::sync::oneshot::channel();
        let _ = err_tx.send(Err(anyhow!("Could not spawn save dialog thread: {err}")));
        return err_rx;
    }
    rx
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::{Path, PathBuf};

    use anyhow::{Context, Result, anyhow};
    use windows::Win32::Foundation::{ERROR_CANCELLED, HWND, S_FALSE, S_OK};
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
        CoInitializeEx, CoTaskMemFree, CoUninitialize,
    };
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::Controls::Dialogs::{
        CommDlgExtendedError, GetOpenFileNameW, GetSaveFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST,
        OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{
        FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_OVERWRITEPROMPT,
        FOS_PATHMUSTEXIST, FileOpenDialog, FileSaveDialog, IFileOpenDialog, IFileSaveDialog,
        IShellItem, SHCreateItemFromParsingName, SIGDN_FILESYSPATH, ShellExecuteW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, SW_SHOWNORMAL,
    };
    use windows::core::{HSTRING, PCWSTR, PWSTR, w};

    use super::{FileDialogFilter, OpenFileRequest, SaveFileRequest};

    struct ComStaGuard {
        should_uninit: bool,
    }

    impl ComStaGuard {
        fn enter() -> Self {
            let hr =
                unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
            // Only uninitialize when we succeeded (S_OK or S_FALSE). Never call
            // CoUninitialize on RPC_E_CHANGED_MODE.
            Self {
                should_uninit: hr == S_OK || hr == S_FALSE,
            }
        }
    }

    impl Drop for ComStaGuard {
        fn drop(&mut self) {
            if self.should_uninit {
                unsafe { CoUninitialize() };
            }
        }
    }

    /// Captures the active/foreground window handle ONLY if it belongs to the
    /// current VanguardCV process (stored as raw `isize` so it can be passed to
    /// the dialog STA thread).
    pub fn current_process_owner_hwnd() -> isize {
        let current_pid = unsafe { GetCurrentProcessId() };
        let candidate = unsafe { GetForegroundWindow() };
        if candidate.0.is_null() {
            return 0;
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(candidate, Some(&mut pid)) };
        if pid == current_pid {
            candidate.0 as isize
        } else {
            0
        }
    }

    fn owner_hwnd(raw: isize) -> Option<HWND> {
        if raw == 0 {
            None
        } else {
            Some(HWND(raw as *mut core::ffi::c_void))
        }
    }

    pub fn open_url(url: &str) -> Result<()> {
        let _com = ComStaGuard::enter();
        let wide_url = HSTRING::from(url);
        let res = unsafe { ShellExecuteW(None, w!("open"), &wide_url, None, None, SW_SHOWNORMAL) };
        if (res.0 as isize) > 32 {
            return Ok(());
        }
        let os_err = std::io::Error::last_os_error();
        tracing::warn!(
            code = res.0 as isize,
            error = %os_err,
            "ShellExecuteW failed to open URL; trying open::that_detached fallback"
        );
        open::that_detached(url).map_err(|fallback_err| {
            anyhow!(
                "Could not open system browser (ShellExecuteW code {}: {os_err}; fallback: {fallback_err})",
                res.0 as isize
            )
        })
    }

    fn is_cancelled(err: &windows::core::Error) -> bool {
        err.code() == ERROR_CANCELLED.to_hresult()
    }

    /// Converts a COM-allocated `PWSTR` returned by `IShellItem::GetDisplayName`
    /// into a `PathBuf` and unconditionally releases it via `CoTaskMemFree`.
    unsafe fn pwstr_to_path_and_free(display: PWSTR) -> Result<PathBuf> {
        let converted = unsafe { display.to_string() };
        unsafe { CoTaskMemFree(Some(display.0.cast())) };
        Ok(PathBuf::from(converted?))
    }

    fn encode_wide_null(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn encode_path_wide_null(path: &Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn build_com_filters(filters: &[FileDialogFilter]) -> Vec<(Vec<u16>, Vec<u16>)> {
        let mut pairs = Vec::with_capacity(filters.len() + 1);
        for f in filters {
            if f.extensions.is_empty() {
                continue;
            }
            let pattern = f
                .extensions
                .iter()
                .map(|ext| format!("*.{}", ext.trim_start_matches('.')))
                .collect::<Vec<_>>()
                .join(";");
            pairs.push((encode_wide_null(f.label), encode_wide_null(&pattern)));
        }
        pairs.push((encode_wide_null("All Files (*.*)"), encode_wide_null("*.*")));
        pairs
    }

    fn build_ofn_filter_buffer(filters: &[FileDialogFilter]) -> Vec<u16> {
        let mut buf = Vec::new();
        for f in filters {
            if f.extensions.is_empty() {
                continue;
            }
            let pattern = f
                .extensions
                .iter()
                .map(|ext| format!("*.{}", ext.trim_start_matches('.')))
                .collect::<Vec<_>>()
                .join(";");
            buf.extend(f.label.encode_utf16());
            buf.push(0);
            buf.extend(pattern.encode_utf16());
            buf.push(0);
        }
        buf.extend("All Files (*.*)".encode_utf16());
        buf.push(0);
        buf.extend("*.*".encode_utf16());
        buf.push(0);
        buf.push(0); // double-NUL terminator
        buf
    }

    fn shell_item_for_dir(dir: &Path) -> Option<IShellItem> {
        if !dir.exists() {
            return None;
        }
        let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        let raw = canonical.to_string_lossy();
        let cleaned = raw.strip_prefix(r"\\?\").unwrap_or(&raw);
        let wide = HSTRING::from(cleaned);
        unsafe { SHCreateItemFromParsingName(&wide, None).ok() }
    }

    pub fn run_open_dialog(request: &OpenFileRequest, owner_raw: isize) -> Result<Option<PathBuf>> {
        let _com = ComStaGuard::enter();
        let owner = owner_hwnd(owner_raw);

        match run_com_open_dialog(request, owner) {
            Ok(path) => Ok(path),
            Err(com_err) => {
                tracing::warn!(
                    error = %com_err,
                    "IFileOpenDialog failed on Windows; falling back to GetOpenFileNameW"
                );
                run_legacy_open_dialog(request, owner)
                    .with_context(|| format!("Native file picker failed (COM: {com_err})"))
            }
        }
    }

    fn run_com_open_dialog(
        request: &OpenFileRequest,
        owner: Option<HWND>,
    ) -> Result<Option<PathBuf>> {
        let dialog: IFileOpenDialog =
            unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)? };

        let filter_buffers = build_com_filters(&request.filters);
        let filter_specs: Vec<COMDLG_FILTERSPEC> = filter_buffers
            .iter()
            .map(|(name, spec)| COMDLG_FILTERSPEC {
                pszName: PCWSTR(name.as_ptr()),
                pszSpec: PCWSTR(spec.as_ptr()),
            })
            .collect();

        unsafe {
            let base_opts = dialog.GetOptions()?;
            dialog.SetOptions(
                base_opts
                    | FOS_FORCEFILESYSTEM
                    | FOS_FILEMUSTEXIST
                    | FOS_PATHMUSTEXIST
                    | FOS_NOCHANGEDIR,
            )?;
            if !request.title.trim().is_empty() {
                dialog.SetTitle(&HSTRING::from(request.title.as_str()))?;
            }
            if !filter_specs.is_empty() {
                dialog.SetFileTypes(&filter_specs)?;
                dialog.SetFileTypeIndex(1)?;
            }
            if let Some(dir) = request.initial_dir.as_deref()
                && let Some(folder_item) = shell_item_for_dir(dir)
            {
                let _ = dialog.SetDefaultFolder(&folder_item);
            }

            if let Err(err) = dialog.Show(owner) {
                if is_cancelled(&err) {
                    return Ok(None);
                }
                return Err(err.into());
            }

            let item = dialog.GetResult()?;
            let display = item.GetDisplayName(SIGDN_FILESYSPATH)?;
            Ok(Some(pwstr_to_path_and_free(display)?))
        }
    }

    fn run_legacy_open_dialog(
        request: &OpenFileRequest,
        owner: Option<HWND>,
    ) -> Result<Option<PathBuf>> {
        let mut file_buf = vec![0u16; 32_768];
        let filter_buf = build_ofn_filter_buffer(&request.filters);
        let title_buf = encode_wide_null(&request.title);
        let initial_dir_buf = request
            .initial_dir
            .as_deref()
            .filter(|d| d.exists())
            .map(encode_path_wide_null);

        let mut ofn = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: owner.unwrap_or_default(),
            lpstrFilter: PCWSTR(filter_buf.as_ptr()),
            nFilterIndex: 1,
            lpstrFile: PWSTR(file_buf.as_mut_ptr()),
            nMaxFile: file_buf.len() as u32,
            lpstrInitialDir: initial_dir_buf
                .as_ref()
                .map_or(PCWSTR::null(), |b| PCWSTR(b.as_ptr())),
            lpstrTitle: if request.title.trim().is_empty() {
                PCWSTR::null()
            } else {
                PCWSTR(title_buf.as_ptr())
            },
            Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
            ..Default::default()
        };

        let ok = unsafe { GetOpenFileNameW(&mut ofn) }.as_bool();
        if ok {
            let len = file_buf
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(file_buf.len());
            let os = OsString::from_wide(&file_buf[..len]);
            Ok(Some(PathBuf::from(os)))
        } else {
            let err_code = unsafe { CommDlgExtendedError() };
            if err_code.0 == 0 {
                Ok(None)
            } else {
                Err(anyhow!("GetOpenFileNameW failed (code 0x{:X})", err_code.0))
            }
        }
    }

    pub fn run_save_dialog(request: &SaveFileRequest, owner_raw: isize) -> Result<Option<PathBuf>> {
        let _com = ComStaGuard::enter();
        let owner = owner_hwnd(owner_raw);

        match run_com_save_dialog(request, owner) {
            Ok(path) => Ok(path),
            Err(com_err) => {
                tracing::warn!(
                    error = %com_err,
                    "IFileSaveDialog failed on Windows; falling back to GetSaveFileNameW"
                );
                run_legacy_save_dialog(request, owner)
                    .with_context(|| format!("Native save dialog failed (COM: {com_err})"))
            }
        }
    }

    fn run_com_save_dialog(
        request: &SaveFileRequest,
        owner: Option<HWND>,
    ) -> Result<Option<PathBuf>> {
        let dialog: IFileSaveDialog =
            unsafe { CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)? };

        let filter_buffers = build_com_filters(&request.filters);
        let filter_specs: Vec<COMDLG_FILTERSPEC> = filter_buffers
            .iter()
            .map(|(name, spec)| COMDLG_FILTERSPEC {
                pszName: PCWSTR(name.as_ptr()),
                pszSpec: PCWSTR(spec.as_ptr()),
            })
            .collect();

        unsafe {
            let base_opts = dialog.GetOptions()?;
            dialog.SetOptions(
                base_opts
                    | FOS_FORCEFILESYSTEM
                    | FOS_PATHMUSTEXIST
                    | FOS_OVERWRITEPROMPT
                    | FOS_NOCHANGEDIR,
            )?;
            if !request.title.trim().is_empty() {
                dialog.SetTitle(&HSTRING::from(request.title.as_str()))?;
            }
            if !filter_specs.is_empty() {
                dialog.SetFileTypes(&filter_specs)?;
                dialog.SetFileTypeIndex(1)?;
            }
            if let Some(ext) = request
                .default_extension
                .as_deref()
                .filter(|e| !e.is_empty())
            {
                let clean = ext.trim_start_matches('.');
                let _ = dialog.SetDefaultExtension(&HSTRING::from(clean));
            }
            if let Some(name) = request.default_name.as_deref().filter(|n| !n.is_empty()) {
                let _ = dialog.SetFileName(&HSTRING::from(name));
            }
            if let Some(dir) = request.initial_dir.as_deref()
                && let Some(folder_item) = shell_item_for_dir(dir)
            {
                let _ = dialog.SetDefaultFolder(&folder_item);
            }

            if let Err(err) = dialog.Show(owner) {
                if is_cancelled(&err) {
                    return Ok(None);
                }
                return Err(err.into());
            }

            let item = dialog.GetResult()?;
            let display = item.GetDisplayName(SIGDN_FILESYSPATH)?;
            Ok(Some(pwstr_to_path_and_free(display)?))
        }
    }

    fn run_legacy_save_dialog(
        request: &SaveFileRequest,
        owner: Option<HWND>,
    ) -> Result<Option<PathBuf>> {
        let mut file_buf = vec![0u16; 32_768];
        if let Some(name) = request.default_name.as_deref() {
            let max_chars = file_buf.len().saturating_sub(1);
            for (dst, src) in file_buf[..max_chars].iter_mut().zip(name.encode_utf16()) {
                *dst = src;
            }
        }
        let filter_buf = build_ofn_filter_buffer(&request.filters);
        let title_buf = encode_wide_null(&request.title);
        let def_ext_buf = request
            .default_extension
            .as_deref()
            .map(|e| encode_wide_null(e.trim_start_matches('.')));
        let initial_dir_buf = request
            .initial_dir
            .as_deref()
            .filter(|d| d.exists())
            .map(encode_path_wide_null);

        let mut ofn = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: owner.unwrap_or_default(),
            lpstrFilter: PCWSTR(filter_buf.as_ptr()),
            nFilterIndex: 1,
            lpstrFile: PWSTR(file_buf.as_mut_ptr()),
            nMaxFile: file_buf.len() as u32,
            lpstrInitialDir: initial_dir_buf
                .as_ref()
                .map_or(PCWSTR::null(), |b| PCWSTR(b.as_ptr())),
            lpstrTitle: if request.title.trim().is_empty() {
                PCWSTR::null()
            } else {
                PCWSTR(title_buf.as_ptr())
            },
            lpstrDefExt: def_ext_buf
                .as_ref()
                .map_or(PCWSTR::null(), |b| PCWSTR(b.as_ptr())),
            Flags: OFN_EXPLORER | OFN_PATHMUSTEXIST | OFN_OVERWRITEPROMPT | OFN_NOCHANGEDIR,
            ..Default::default()
        };

        let ok = unsafe { GetSaveFileNameW(&mut ofn) }.as_bool();
        if ok {
            let len = file_buf
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(file_buf.len());
            let os = OsString::from_wide(&file_buf[..len]);
            Ok(Some(PathBuf::from(os)))
        } else {
            let err_code = unsafe { CommDlgExtendedError() };
            if err_code.0 == 0 {
                Ok(None)
            } else {
                Err(anyhow!("GetSaveFileNameW failed (code 0x{:X})", err_code.0))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_external_url_rejects_empty_and_non_http_schemes() {
        assert!(open_external_url("").is_err());
        assert!(open_external_url("   ").is_err());
        assert!(open_external_url("file:///C:/Windows/win.ini").is_err());
        assert!(open_external_url("javascript:alert(1)").is_err());
        assert!(open_external_url("https://example.com\r\n--bad").is_err());
        assert!(open_external_url("https://").is_err());
    }
}
