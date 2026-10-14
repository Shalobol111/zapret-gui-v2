// Манифест Windows. В релизе программа всегда запрашивает права администратора
// (winws.exe и службы без них не работают). В отладке — asInvoker: `tauri dev`
// запускается из обычного терминала и не смог бы стартовать процесс с
// requireAdministrator (ERROR_ELEVATION_REQUIRED); интерфейс в этом случае
// показывает плашку «Без прав администратора».
//
// Зависимость от Common Controls v6 обязательна: tauri-build добавляет её в свой
// манифест по умолчанию, а при замене манифеста её нужно указать самим.

const MANIFEST_TEMPLATE: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*" />
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="{LEVEL}" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}" />
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>
"#;

fn main() {
    let release = std::env::var("PROFILE").as_deref() == Ok("release");
    let level = if release { "requireAdministrator" } else { "asInvoker" };
    let manifest = MANIFEST_TEMPLATE.replace("{LEVEL}", level);

    let attrs =
        tauri_build::Attributes::new().windows_attributes(tauri_build::WindowsAttributes::new().app_manifest(manifest));
    tauri_build::try_build(attrs).expect("tauri-build failed");
}
