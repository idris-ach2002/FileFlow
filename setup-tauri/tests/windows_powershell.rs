#[cfg(windows)]
mod windows_tests {
    use std::{
        env, fs,
        path::Path,
        process::{Command, Output},
        time::{SystemTime, UNIX_EPOCH},
    };

    const AUTHENTICODE_SCRIPT: &str = "& { param([string]$Path); \
         (Get-AuthenticodeSignature -LiteralPath $Path).Status.ToString() }";

    const SHORTCUT_SCRIPT: &str = "& { param([string]$Target,[string]$Shortcut,[string]$WorkingDirectory); \
         $shell=New-Object -ComObject WScript.Shell; \
         $link=$shell.CreateShortcut($Shortcut); \
         $link.TargetPath=$Target; \
         $link.WorkingDirectory=$WorkingDirectory; \
         $link.IconLocation=\"$Target,0\"; \
         $link.Description='FileFlow Windows native regression test'; \
         $link.Save() }";

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

    fn powershell(script: &str, args: &[&Path]) -> Output {
        let mut command = Command::new("powershell.exe");

        command.args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ]);

        for arg in args {
            command.arg(arg);
        }

        command.output().expect("launch Windows PowerShell")
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

        let output = powershell(AUTHENTICODE_SCRIPT, &[target.as_path()]);

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
            &[target.as_path(), shortcut.as_path(), app_dir.as_path()],
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
