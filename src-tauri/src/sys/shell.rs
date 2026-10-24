//! Открыть файл или ссылку от имени пользователя, а не с правами администратора ZapretGUI.
//!
//! Команду выполняет сам рабочий стол — процесс проводника пользователя — через
//! IShellDispatch2::ShellExecute (способ из блога Раймонда Чена «How can I launch an unelevated
//! process from my elevated process»). В отличие от `explorer.exe <аргумент>` ссылка уходит как
//! есть: адрес вида `tg://proxy?server=…&port=…` проводник в командной строке не узнаёт и
//! открывает «Документы».

use windows::core::{Interface, BSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IDispatch, IServiceProvider, CLSCTX_LOCAL_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::{
    IShellBrowser, IShellDispatch2, IShellFolderViewDual, IShellView, IShellWindows, SID_STopLevelBrowser,
    ShellWindows, CSIDL_DESKTOP, SVGIO_BACKGROUND, SWC_DESKTOP, SWFO_NEEDDISPATCH,
};

fn shell_execute(target: &str) -> windows::core::Result<()> {
    unsafe {
        let windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER)?;
        let mut hwnd = 0i32;
        let desktop: IDispatch = windows.FindWindowSW(
            &VARIANT::from(CSIDL_DESKTOP as i32),
            &VARIANT::default(),
            SWC_DESKTOP,
            &mut hwnd,
            SWFO_NEEDDISPATCH,
        )?;
        let browser: IShellBrowser = desktop.cast::<IServiceProvider>()?.QueryService(&SID_STopLevelBrowser)?;
        let view: IShellView = browser.QueryActiveShellView()?;
        let folder: IShellFolderViewDual = view.GetItemObject::<IDispatch>(SVGIO_BACKGROUND)?.cast()?;
        let shell: IShellDispatch2 = folder.Application()?.cast()?;
        let empty = VARIANT::default();
        shell.ShellExecute(&BSTR::from(target), &empty, &empty, &empty, &empty)
    }
}

/// Открыть `target` (ссылку или путь) обработчиком по умолчанию с правами пользователя.
/// COM — в отдельном потоке (нужен однопоточный апартамент, независимо от вызывающего потока).
pub fn open_unelevated(target: &str) -> Result<(), String> {
    let target = target.to_string();
    std::thread::spawn(move || unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let r = shell_execute(&target);
        if init.is_ok() {
            CoUninitialize();
        }
        r
    })
    .join()
    .map_err(|_| "сбой при открытии ссылки".to_string())?
    .map_err(|e| format!("проводник не открыл ссылку: {}", e.message()))
}

#[cfg(test)]
mod tests {
    /// Механизм: рабочий стол запускает .cmd, который создаёт файл-отметку.
    /// `cargo test -- --ignored live_shell_execute_file --nocapture`
    #[test]
    #[ignore]
    fn live_shell_execute_file() {
        let dir = std::env::temp_dir();
        let mark = dir.join("zgui-shell-mark.txt");
        let script = dir.join("zgui-shell-test.cmd");
        let _ = std::fs::remove_file(&mark);
        std::fs::write(&script, format!("@echo ok> \"{}\"\r\n", mark.display())).unwrap();
        super::open_unelevated(&script.display().to_string()).unwrap();
        let t0 = std::time::Instant::now();
        while !mark.exists() {
            assert!(t0.elapsed().as_secs() < 10, "рабочий стол не запустил файл");
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        println!("файл запущен рабочим столом за {:.1} с", t0.elapsed().as_secs_f32());
        let _ = std::fs::remove_file(&mark);
        let _ = std::fs::remove_file(&script);
    }

    /// Ссылка с `?` и `&` доходит до обработчика целиком: браузер по умолчанию открывает
    /// http://127.0.0.1:<порт>/…?server=…&port=…, тест принимает запрос и сверяет адрес.
    /// `cargo test -- --ignored live_shell_execute_url --nocapture`
    #[test]
    #[ignore]
    fn live_shell_execute_url() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{port}/zgui-test?server=127.0.0.1&port=1443&secret=dd0123456789abcdef");
        super::open_unelevated(&url).unwrap();
        listener.set_nonblocking(false).unwrap();
        let (mut sock, _) = listener.accept().unwrap();
        let mut buf = [0u8; 2048];
        let n = sock.read(&mut buf).unwrap();
        let req = String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or_default().to_string();
        let body = "<meta charset=utf-8><body style='font:16px sans-serif;padding:40px'>Тест ZapretGUI — вкладку можно закрыть.";
        let head =
            ["HTTP/1.1 200 OK", "Content-Type: text/html; charset=utf-8", "Connection: close", "", ""].join("\r\n");
        let _ = write!(sock, "{head}{body}");
        println!("браузер запросил: {req}");
        assert!(req.contains("/zgui-test?server=127.0.0.1&port=1443&secret=dd0123456789abcdef"), "{req}");
    }
}
