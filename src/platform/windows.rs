//! Windows handles, registry, shell integration and TCP table inspection.
//!
//! FFI safety: wide() creates NUL-terminated strings whose owners live through each
//! synchronous call; output pointers refer to live initialized/aligned storage.
//! Successful owned handles have one RAII owner, are checked before use and closed
//! exactly once. OS pseudo-handles/registry roots are borrowed, never closed here.
//! Variable-size buffers and POD reads have additional local bounds checks below.
use anyhow::{bail, Context, Result};
use std::{ffi::OsStr, os::windows::ffi::OsStrExt, path::Path};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::*,
    System::{JobObjects::*, Threading::*},
    UI::{Shell::*, WindowsAndMessaging::*},
};

pub fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}
fn tcp_listeners(family: u32) -> Result<Vec<(u16, u32)>> {
    use windows_sys::Win32::NetworkManagement::IpHelper::*;
    let mut size = 0u32;
    let status = unsafe {
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            0,
            family,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if status != ERROR_INSUFFICIENT_BUFFER && status != NO_ERROR {
        return Err(std::io::Error::from_raw_os_error(status as i32).into());
    }
    // The table can grow between calls. Retry using Windows' new required size.
    for _ in 0..4 {
        if !(4..=16_777_216).contains(&size) {
            bail!("Недопустимый размер TCP table");
        }
        let mut storage = vec![0u32; (size as usize).div_ceil(4)];
        let capacity = storage.len() * 4;
        let status = unsafe {
            GetExtendedTcpTable(
                storage.as_mut_ptr().cast(),
                &mut size,
                0,
                family,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if status == ERROR_INSUFFICIENT_BUFFER {
            continue;
        }
        if status != NO_ERROR {
            return Err(std::io::Error::from_raw_os_error(status as i32).into());
        }
        if size as usize > capacity {
            bail!("TCP table превышает выделенный буфер");
        }
        // SAFETY: size was checked against the initialized u32 backing allocation.
        let bytes =
            unsafe { std::slice::from_raw_parts(storage.as_ptr().cast::<u8>(), size as usize) };
        return parse_tcp_listeners(bytes, family);
    }
    bail!("TCP table меняется слишком быстро; повторите проверку")
}
fn parse_tcp_listeners(bytes: &[u8], family: u32) -> Result<Vec<(u16, u32)>> {
    use windows_sys::Win32::NetworkManagement::IpHelper::*;
    let count = u32::from_ne_bytes(
        bytes
            .get(..4)
            .context("Нет заголовка TCP table")?
            .try_into()?,
    ) as usize;
    let row_size = match family {
        2 => std::mem::size_of::<MIB_TCPROW_OWNER_PID>(),
        23 => std::mem::size_of::<MIB_TCP6ROW_OWNER_PID>(),
        _ => bail!("Неизвестное семейство TCP table"),
    };
    if count > (bytes.len() - 4) / row_size {
        bail!("Усечённая TCP table");
    }
    let mut listeners = Vec::with_capacity(count);
    for index in 0..count {
        // SAFETY: the header/count check bounds every row; read_unaligned handles byte slices.
        let pointer = unsafe { bytes.as_ptr().add(4 + index * row_size) };
        let (port, pid) = if family == 2 {
            let row = unsafe { std::ptr::read_unaligned(pointer.cast::<MIB_TCPROW_OWNER_PID>()) };
            (row.dwLocalPort, row.dwOwningPid)
        } else {
            let row = unsafe { std::ptr::read_unaligned(pointer.cast::<MIB_TCP6ROW_OWNER_PID>()) };
            (row.dwLocalPort, row.dwOwningPid)
        };
        listeners.push((u16::from_be(port as u16), pid));
    }
    Ok(listeners)
}
pub fn port_available(port: u16) -> Result<bool> {
    // AF_INET / AF_INET6. Include IPv6 wildcard listeners, which can be dual-stack.
    for family in [2, 23] {
        if tcp_listeners(family)?
            .iter()
            .any(|(local, _)| *local == port)
        {
            return Ok(false);
        }
    }
    Ok(true)
}
pub fn port_owner(port: u16) -> Option<String> {
    for family in [2, 23] {
        let Some((_, pid)) = tcp_listeners(family)
            .ok()?
            .into_iter()
            .find(|(local, _)| *local == port)
        else {
            continue;
        };
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        let name = if process.is_null() {
            None
        } else {
            let mut name = vec![0u16; 32768];
            let mut length = name.len() as u32;
            let success =
                unsafe { QueryFullProcessImageNameW(process, 0, name.as_mut_ptr(), &mut length) }
                    != 0;
            unsafe {
                CloseHandle(process);
            }
            if success {
                std::path::PathBuf::from(String::from_utf16_lossy(&name[..length as usize]))
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
            } else {
                None
            }
        };
        return Some(format!(
            "{}PID {}",
            name.map(|n| format!("{n}, ")).unwrap_or_default(),
            pid
        ));
    }
    None
}
pub fn replace_file(from: &Path, to: &Path) -> Result<()> {
    if unsafe {
        MoveFileExW(
            wide(from).as_ptr(),
            wide(to).as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

pub struct InstanceGuard(HANDLE);
impl InstanceGuard {
    pub fn acquire(id: &str, title: &str) -> Result<Option<Self>> {
        let name = wide(format!("Local\\HHClickerRust-{id}"));
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                CloseHandle(handle);
                let hwnd = FindWindowW(std::ptr::null(), wide(title).as_ptr());
                if !hwnd.is_null() {
                    ShowWindow(hwnd, SW_RESTORE);
                    SetForegroundWindow(hwnd);
                }
            }
            return Ok(None);
        }
        Ok(Some(Self(handle)))
    }
}
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

// A job owns only children explicitly assigned by this launcher. Parent death closes it,
// so there is no unsafe PID-based recovery and no agent driving a browser unattended.
pub struct ChildJob(HANDLE);
// SAFETY: a job HANDLE can cross threads; this value retains unique CloseHandle ownership.
unsafe impl Send for ChildJob {}
impl ChildJob {
    pub fn new() -> Result<Self> {
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let job = Self(handle);
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of_val(&info) as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(job)
    }
    pub fn assign(&self, child: &std::process::Child) -> Result<()> {
        use std::os::windows::io::AsRawHandle;
        if unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
}
impl Drop for ChildJob {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub fn open_url(url: &str) -> Result<()> {
    // Opening an arbitrary protocol can execute local applications. Allow only explicit web URLs.
    if !(url.starts_with("https://") || url.starts_with("http://127.0.0.1:"))
        || url.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        bail!("Недопустимая ссылка");
    }
    shell_open(url)
}
fn shell_open(value: &str) -> Result<()> {
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            wide("open").as_ptr(),
            wide(value).as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    if result <= 32 {
        bail!("Windows не смог открыть ссылку: {result}");
    }
    Ok(())
}
pub fn open_folder(path: &Path) -> Result<()> {
    shell_open(path.to_str().context("Путь не является Unicode")?)
}
// Fixed script; paths travel only through child environment variables. In particular
// [char]34 avoids a second round of command-line quote interpretation.
const MSI_ARGUMENTS: &str = "$q=[char]34; $arguments=@('/i',($q+$env:HH_RUST_PACKAGE+$q),'/qn','/norestart','/L*v',($q+$env:HH_RUST_MSI_LOG+$q));";

/// Match the reference launcher's elevated, quiet Windows Installer flow.
pub fn install_msi(package: &Path, log: &Path, timeout: std::time::Duration) -> Result<()> {
    let package = std::path::absolute(package)?;
    let log = std::path::absolute(log)?;
    std::fs::create_dir_all(log.parent().context("Каталог MSI log")?)?;
    let msiexec = std::env::var_os("SystemRoot")
        .or_else(|| std::env::var_os("WINDIR"))
        .map(|p| std::path::PathBuf::from(p).join("System32/msiexec.exe"))
        .filter(|p| p.is_absolute() && p.is_file())
        .context("Системный Windows Installer не найден")?;
    let script = format!("$ErrorActionPreference='Stop'; {MSI_ARGUMENTS} try {{$p=Start-Process -FilePath $env:HH_RUST_MSIEXEC -ArgumentList $arguments -Verb RunAs -Wait -PassThru}} catch {{$e=$_.Exception; if($e -is [ComponentModel.Win32Exception]){{$n=$e.NativeErrorCode}} elseif($e.InnerException -is [ComponentModel.Win32Exception]){{$n=$e.InnerException.NativeErrorCode}} else {{$n=0}}; if($n -eq 1223){{Write-Output 'HH_MSI_CANCELLED'}} else {{Write-Output ('HH_MSI_ELEVATION_FAILED='+$n)}}; exit 0}}; Write-Output ('HH_MSI_EXIT='+$p.ExitCode)");
    let output = crate::process::powershell(
        &script,
        &[
            ("HH_RUST_MSIEXEC", msiexec.as_os_str()),
            ("HH_RUST_PACKAGE", package.as_os_str()),
            ("HH_RUST_MSI_LOG", log.as_os_str()),
        ],
        timeout,
    )?;
    if output.contains("HH_MSI_CANCELLED") {
        bail!("Установка Chrome отменена в запросе Windows");
    }
    let exit = output.lines().find_map(|line| line.trim().strip_prefix("HH_MSI_EXIT=").and_then(|v| v.parse::<u32>().ok()))
        .context("Не удалось подтвердить запуск Windows Installer. Проверьте запрос Windows и журнал установки")?;
    match exit {
        0 | 3010 => Ok(()),
        1602 => bail!("Установка Chrome отменена. Журнал: {}", log.display()),
        1618 => bail!("Уже выполняется другая установка Windows. Дождитесь её завершения и повторите. Журнал: {}", log.display()),
        code => bail!(
            "Установка Chrome: MSI exit {code}. Журнал: {}",
            log.display()
        ),
    }
}
pub fn startup_error(message: &str) {
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide(message).as_ptr(),
            wide("HH Clicker Rust").as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}
pub fn product_version(path: &Path) -> Result<[u32; 3]> {
    let path = wide(path);
    let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), std::ptr::null_mut()) };
    if size == 0 || size > 16_777_216 {
        bail!("Не удалось прочитать версию EXE");
    }
    let mut storage = vec![0u32; (size as usize).div_ceil(4)];
    if unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, storage.as_mut_ptr().cast()) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut pointer = std::ptr::null_mut();
    let mut length = 0;
    if unsafe {
        VerQueryValueW(
            storage.as_ptr().cast(),
            wide("\\").as_ptr(),
            &mut pointer,
            &mut length,
        )
    } == 0
        || pointer.is_null()
        || length < std::mem::size_of::<VS_FIXEDFILEINFO>() as u32
    {
        bail!("Нет ProductVersion в EXE");
    }
    // SAFETY: VerQueryValueW returned a non-null record of sufficient length;
    // storage remains live until after the copy. The structure is POD.
    let value = unsafe { std::ptr::read_unaligned(pointer.cast::<VS_FIXEDFILEINFO>()) };
    if value.dwSignature != 0xfeef04bd {
        bail!("Некорректный ProductVersion");
    }
    Ok([
        value.dwProductVersionMS >> 16,
        value.dwProductVersionMS & 0xffff,
        value.dwProductVersionLS >> 16,
    ])
}
pub fn registry_dword(machine: bool, key: &str, name: &str) -> Option<u32> {
    use windows_sys::Win32::System::Registry::*;
    let mut value = 0u32;
    let mut size = 4u32;
    let root = if machine {
        HKEY_LOCAL_MACHINE
    } else {
        HKEY_CURRENT_USER
    };
    let status = unsafe {
        RegGetValueW(
            root,
            wide(key).as_ptr(),
            wide(name).as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut value as *mut u32).cast(),
            &mut size,
        )
    };
    (status == ERROR_SUCCESS && size == 4).then_some(value)
}
pub fn registry_string(machine: bool, key: &str, name: &str) -> Option<String> {
    use windows_sys::Win32::System::Registry::*;
    let root = if machine {
        HKEY_LOCAL_MACHINE
    } else {
        HKEY_CURRENT_USER
    };
    let (key, name) = (wide(key), wide(name));
    let mut size = 0;
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_SUBKEY_WOW6464KEY;
    if unsafe {
        RegGetValueW(
            root,
            key.as_ptr(),
            name.as_ptr(),
            flags,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    } != 0
        || size > 65536
    {
        return None;
    }
    let mut buffer = vec![0u16; (size as usize / 2) + 1];
    if unsafe {
        RegGetValueW(
            root,
            key.as_ptr(),
            name.as_ptr(),
            flags,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    } != 0
    {
        return None;
    }
    Some(String::from_utf16_lossy(
        &buffer[..buffer.iter().position(|v| *v == 0).unwrap_or(buffer.len())],
    ))
}
pub fn local_date(seconds: u64) -> Option<String> {
    let ticks = seconds
        .checked_add(11_644_473_600)?
        .checked_mul(10_000_000)?;
    let file = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut local: FILETIME = unsafe { std::mem::zeroed() };
    let mut system: SYSTEMTIME = unsafe { std::mem::zeroed() };
    if unsafe { FileTimeToLocalFileTime(&file, &mut local) } == 0
        || unsafe { windows_sys::Win32::System::Time::FileTimeToSystemTime(&local, &mut system) }
            == 0
    {
        return None;
    }
    Some(format!(
        "{:02}.{:02} {:02}:{:02}",
        system.wDay, system.wMonth, system.wHour, system.wMinute
    ))
}
pub fn virtualization_features_available() -> bool {
    // SAFETY: the function takes documented constant IDs and no borrowed pointers.
    unsafe {
        IsProcessorFeaturePresent(PF_VIRT_FIRMWARE_ENABLED) != 0
            && IsProcessorFeaturePresent(PF_SECOND_LEVEL_ADDRESS_TRANSLATION) != 0
    }
}

pub fn local_checked_at(value: &str) -> Option<String> {
    if !value.ends_with('Z') || value.len() < 20 {
        return None;
    }
    let date = value.get(..19)?;
    let numbers: Vec<u16> = date
        .split(['-', 'T', ':'])
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()
        .ok()?;
    if numbers.len() != 6 {
        return None;
    }
    let system = windows_sys::Win32::Foundation::SYSTEMTIME {
        wYear: numbers[0],
        wMonth: numbers[1],
        wDay: numbers[2],
        wHour: numbers[3],
        wMinute: numbers[4],
        wSecond: numbers[5],
        wDayOfWeek: 0,
        wMilliseconds: 0,
    };
    let mut file = windows_sys::Win32::Foundation::FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    // SAFETY: both SYSTEMTIME and FILETIME are initialized, live POD values.
    if unsafe { windows_sys::Win32::System::Time::SystemTimeToFileTime(&system, &mut file) } == 0 {
        return None;
    }
    let ticks = ((file.dwHighDateTime as u64) << 32) | file.dwLowDateTime as u64;
    local_date((ticks / 10_000_000).checked_sub(11_644_473_600)?)
}

#[cfg(test)]
mod tests {
    #[test]
    fn tcp_listener_tables_decode_ipv4_ipv6_and_reject_truncation() {
        use windows_sys::Win32::NetworkManagement::IpHelper::*;
        let mut ipv4: MIB_TCPROW_OWNER_PID = unsafe { std::mem::zeroed() };
        ipv4.dwLocalPort = u16::to_be(28080) as u32;
        ipv4.dwOwningPid = 1234;
        let mut ipv6: MIB_TCP6ROW_OWNER_PID = unsafe { std::mem::zeroed() };
        ipv6.dwLocalPort = u16::to_be(23000) as u32;
        ipv6.dwOwningPid = 5678;
        for (family, row, size, expected) in [
            (
                2,
                (&ipv4 as *const MIB_TCPROW_OWNER_PID).cast::<u8>(),
                std::mem::size_of_val(&ipv4),
                (28080, 1234),
            ),
            (
                23,
                (&ipv6 as *const MIB_TCP6ROW_OWNER_PID).cast::<u8>(),
                std::mem::size_of_val(&ipv6),
                (23000, 5678),
            ),
        ] {
            let mut bytes = 1u32.to_ne_bytes().to_vec();
            bytes.extend_from_slice(unsafe { std::slice::from_raw_parts(row, size) });
            assert_eq!(
                super::parse_tcp_listeners(&bytes, family).unwrap(),
                vec![expected]
            );
            bytes.pop();
            assert!(super::parse_tcp_listeners(&bytes, family).is_err());
        }
        assert!(super::parse_tcp_listeners(&[], 2).is_err());
        assert!(super::parse_tcp_listeners(&0u32.to_ne_bytes(), 23)
            .unwrap()
            .is_empty());
    }
    #[test]
    fn ipv6_listener_is_detected_without_creating_another_socket() {
        let listener = std::net::TcpListener::bind(("::1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(!super::port_available(port).unwrap());
        assert!(super::port_owner(port)
            .unwrap()
            .contains(&format!("PID {}", std::process::id())));
    }
    #[test]
    fn quiet_msi_arguments_preserve_paths_without_executing_installer() {
        let package = std::path::PathBuf::from("D:\\Папка O'Brien $(literal)\\Chrome.msi");
        let log = std::path::PathBuf::from("D:\\Папка с пробелами\\chrome-install.log");
        let script = format!(
            "{} ConvertTo-Json -InputObject $arguments -Compress",
            super::MSI_ARGUMENTS
        );
        let output = crate::process::powershell(
            &script,
            &[
                ("HH_RUST_PACKAGE", package.as_os_str()),
                ("HH_RUST_MSI_LOG", log.as_os_str()),
            ],
            std::time::Duration::from_secs(10),
        )
        .unwrap();
        let args: Vec<String> = serde_json::from_str(&output).unwrap();
        assert_eq!(
            args,
            vec![
                "/i".to_string(),
                format!("\"{}\"", package.display()),
                "/qn".into(),
                "/norestart".into(),
                "/L*v".into(),
                format!("\"{}\"", log.display())
            ]
        );
    }
    use super::*;
    #[test]
    fn listening_port_is_attributed_without_mutating_its_process() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let owner = port_owner(listener.local_addr().unwrap().port()).unwrap();
        assert!(owner.contains(&format!("PID {}", std::process::id())));
    }
    #[test]
    fn only_one_instance_per_identity() {
        let id = uuid::Uuid::new_v4().to_string();
        let a = InstanceGuard::acquire(&id, "test-window-never-created")
            .unwrap()
            .unwrap();
        assert!(InstanceGuard::acquire(&id, "test-window-never-created")
            .unwrap()
            .is_none());
        drop(a);
        assert!(InstanceGuard::acquire(&id, "test-window-never-created")
            .unwrap()
            .is_some());
    }
    #[test]
    fn dangerous_external_schemes_rejected() {
        for url in ["file:///C:/x.exe", "javascript:alert(1)", "ms-settings:"] {
            assert!(open_url(url).is_err());
        }
    }
}
