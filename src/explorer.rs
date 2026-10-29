use crate::process::{encoded_powershell, hide_window, ps_quote};
use anyhow::{Context, Result, bail};
use std::{path::Path, process::Command};

const MENU_KEYS: [(&str, &str); 2] = [
    (r"Software\Classes\Directory\shell\PiBoxComposer", "%1"),
    (
        r"Software\Classes\Directory\Background\shell\PiBoxComposer",
        "%V",
    ),
];

fn registration_script(exe: &Path, data_dir: &Path, install: bool) -> Result<String> {
    if !exe.is_absolute() || !data_dir.is_absolute() {
        bail!("Пути приложения и настроек должны быть абсолютными.");
    }
    let exe = exe.to_string_lossy();
    let data = data_dir.to_string_lossy();
    if exe.contains(['"', '\0', '\r', '\n']) || data.contains(['"', '\0', '\r', '\n']) {
        bail!("Путь содержит недопустимые символы.");
    }
    let mut script = String::from(
        "$ErrorActionPreference = 'Stop'\n[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)\n",
    );
    for (key, placeholder) in MENU_KEYS {
        if !install {
            script.push_str(&format!(
                "[Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree({}, $false)\n",
                ps_quote(key)
            ));
            continue;
        }
        // Appending \. prevents a drive root's final backslash escaping the closing quote.
        // Explorer launches the executable directly; folder names are never shell code.
        let command = format!(r#""{exe}" --data-dir "{data}\." --open "{placeholder}\.""#);
        script.push_str(&format!(
            r#"
$key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey({key})
try {{
    $key.SetValue('', 'Открыть в pi', [Microsoft.Win32.RegistryValueKind]::String)
    $key.SetValue('Icon', {icon}, [Microsoft.Win32.RegistryValueKind]::String)
    $key.SetValue('MultiSelectModel', 'Single', [Microsoft.Win32.RegistryValueKind]::String)
    $child = $key.CreateSubKey('command')
    try {{ $child.SetValue('', {command}, [Microsoft.Win32.RegistryValueKind]::String) }}
    finally {{ $child.Dispose() }}
}} finally {{ $key.Dispose() }}
"#,
            key = ps_quote(key),
            icon = ps_quote(&format!(r#""{exe}",0"#)),
            command = ps_quote(&command)
        ));
    }
    Ok(script)
}

pub fn configure(data_dir: &Path, install: bool) -> Result<()> {
    let exe = std::env::current_exe().context("Не удалось определить путь приложения")?;
    let script = registration_script(&exe, data_dir, install)?;
    let windows = std::env::var_os("SystemRoot").context("Интеграция доступна только в Windows")?;
    let mut command =
        Command::new(Path::new(&windows).join("System32/WindowsPowerShell/v1.0/powershell.exe"));
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-EncodedCommand",
        &encoded_powershell(&script),
    ]);
    hide_window(&mut command);
    let output = command
        .output()
        .context("Не удалось изменить меню Проводника")?;
    if !output.status.success() {
        bail!(
            "Не удалось изменить меню Проводника: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn registration_quotes_paths_and_removal_targets_only_our_verbs() {
        let exe = Path::new(r"C:\Tools\O'Brien $pi\pi-box-composer.exe");
        let data = Path::new(r"C:\User data");
        let script = registration_script(exe, data, true).unwrap();
        assert!(script.contains(r#""C:\Tools\O''Brien $pi\pi-box-composer.exe" --data-dir "C:\User data\." --open "%1\.""#));
        assert!(script.contains(r#"--open "%V\.""#));
        assert!(!script.contains("Invoke-Expression"));
        let remove = registration_script(exe, data, false).unwrap();
        assert_eq!(remove.matches("DeleteSubKeyTree").count(), 2);
        assert!(!remove.contains("CreateSubKey"));
        for (key, _) in MENU_KEYS {
            assert!(remove.contains(&ps_quote(key)));
        }
    }
}
