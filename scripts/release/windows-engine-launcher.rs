use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

fn replace_pack(value: &str, pack: &Path) -> OsString {
    if let Some(rest) = value.strip_prefix("{PACK}/") {
        return pack.join(rest.replace('/', "\\")).into_os_string();
    }
    OsString::from(value)
}

fn prepend_path(entries: &[PathBuf]) {
    let mut values: Vec<PathBuf> = entries
        .iter()
        .filter(|p| p.is_dir())
        .cloned()
        .collect();
    if let Some(existing) = env::var_os("PATH") {
        values.extend(env::split_paths(&existing));
    }
    if let Ok(joined) = env::join_paths(values) {
        env::set_var("PATH", joined);
    }
}

fn main() -> ExitCode {
    let me = match env::current_exe() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("FileFlow engine launcher: current_exe: {e}");
            return ExitCode::from(111);
        }
    };
    let bin = match me.parent() {
        Some(v) => v,
        None => return ExitCode::from(112),
    };
    let pack = match bin.parent() {
        Some(v) => v,
        None => return ExitCode::from(113),
    };
    let stem = me
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or("");
    let spec = bin.join(format!("{stem}.target"));
    let text = match fs::read_to_string(&spec) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "FileFlow engine launcher: {}: {e}",
                spec.display()
            );
            return ExitCode::from(114);
        }
    };
    let mut lines = text
        .lines()
        .filter(|line| !line.trim().is_empty());
    let target_line = match lines.next() {
        Some(v) => v.trim(),
        None => return ExitCode::from(115),
    };
    let target = if let Some(rest) =
        target_line.strip_prefix("{PACK}/")
    {
        pack.join(rest.replace('/', "\\"))
    } else {
        PathBuf::from(target_line)
    };

    let runtime = pack.join("share").join("runtime");
    let office = pack
        .join("share")
        .join("libreoffice")
        .join("program");
    prepend_path(&[
        runtime.clone(),
        runtime.join("Library").join("bin"),
        runtime.join("Scripts"),
        runtime.join("DLLs"),
        office,
    ]);
    env::set_var("PYTHONHOME", &runtime);

    for tess in [
        runtime.join("share").join("tessdata"),
        runtime
            .join("Library")
            .join("share")
            .join("tessdata"),
    ] {
        if tess.is_dir() {
            env::set_var("TESSDATA_PREFIX", tess);
            break;
        }
    }

    let mut command = Command::new(&target);
    for line in lines {
        command.arg(replace_pack(line.trim(), pack));
    }
    command.args(env::args_os().skip(1));

    match command.status() {
        Ok(status) => {
            ExitCode::from(status.code().unwrap_or(1) as u8)
        }
        Err(e) => {
            eprintln!(
                "FileFlow engine launcher: {}: {e}",
                target.display()
            );
            ExitCode::from(116)
        }
    }
}
