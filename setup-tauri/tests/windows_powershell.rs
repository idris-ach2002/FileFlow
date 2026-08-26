#[cfg(windows)]
mod windows_tests {
    use std::{
        env, fs,
        path::Path,
        process::{Command, Output},
        time::{SystemTime, UNIX_EPOCH},
    };

    const AUTHENTICODE_SCRIPT: &str = "$ErrorActionPreference='Stop'; \
         Import-Module Microsoft.PowerShell.Security -ErrorAction Stop; \
         (Get-AuthenticodeSignature -LiteralPath $env:FILEFLOW_PS_PATH).Status.ToString()";

    const SHORTCUT_SCRIPT: &str = "$ErrorActionPreference='Stop'; \
         $Target=$env:FILEFLOW_PS_TARGET; \
         $Shortcut=$env:FILEFLOW_PS_SHORTCUT; \
         $WorkingDirectory=$env:FILEFLOW_PS_WORKING_DIRECTORY; \
         $shell=New-Object -ComObject WScript.Shell; \
         $link=$shell.CreateShortcut($Shortcut); \
         $link.TargetPath=$Target; \
         $link.WorkingDirectory=$WorkingDirectory; \
         $link.IconLocation=($Target + ',0'); \
         $link.Description='FileFlow Windows native regression test'; \
         $link.Save(); \
         if (-not (Test-Path -LiteralPath $Shortcut)) { throw 'shortcut missing' }";

    fn test_directory(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();

        let path = env::temp_dir().join(format!(
            "FileFlow Windows test é space {} {} {}",
            name,
            std::process::id(),
            nanos
        ));

        fs::create_dir_all(&path).expect("create temporary directory");

        path
    }

    fn powershell_program() -> &'static str {
        if Command::new("pwsh.exe")
            .args(["-NoLogo", "-NoProfile", "-Command", "exit 0"])
            .status()
            .is_ok_and(|status| status.success())
        {
            "pwsh.exe"
        } else {
            "powershell.exe"
        }
    }

    fn powershell(script: &str, environment: &[(&str, &Path)]) -> Output {
        let mut command = Command::new(powershell_program());

        command.args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ]);

        for &(name, path) in environment {
            command.env(name, path);
        }

        command.output().expect("launch PowerShell")
    }

    fn failure(output: &Output) -> String {
        format!(
            "\nstatus={}\nstdout={}\nstderr={}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        )
    }

    #[test]
    fn authenticode_accepts_native_windows_path_argument() {
        let dir = test_directory("authenticode");

        let target = dir.join("FileFlow application test é space.exe");

        fs::copy(
            env::current_exe().expect("current test executable"),
            &target,
        )
        .expect("copy executable");

        let output = powershell(
            AUTHENTICODE_SCRIPT,
            &[("FILEFLOW_PS_PATH", target.as_path())],
        );

        assert!(
            output.status.success(),
            "PowerShell Authenticode invocation failed: {}",
            failure(&output),
        );

        let stdout = String::from_utf8_lossy(&output.stdout);

        assert!(
            !stdout.trim().is_empty(),
            "Authenticode returned no status: {}",
            failure(&output),
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn start_menu_shortcut_accepts_native_windows_path_arguments() {
        let dir = test_directory("shortcut");

        let app_dir = dir.join("Application FileFlow é space");

        fs::create_dir_all(&app_dir).expect("create app directory");

        let target = app_dir.join("FileFlow é test.exe");

        fs::copy(
            env::current_exe().expect("current test executable"),
            &target,
        )
        .expect("copy executable");

        let shortcut = dir.join("FileFlow é test.lnk");

        let output = powershell(
            SHORTCUT_SCRIPT,
            &[
                ("FILEFLOW_PS_TARGET", target.as_path()),
                ("FILEFLOW_PS_SHORTCUT", shortcut.as_path()),
                ("FILEFLOW_PS_WORKING_DIRECTORY", app_dir.as_path()),
            ],
        );

        assert!(
            output.status.success(),
            "PowerShell shortcut invocation failed: {}",
            failure(&output),
        );

        assert!(
            shortcut.is_file(),
            "PowerShell returned success but shortcut is absent"
        );

        let _ = fs::remove_dir_all(dir);
    }
}
