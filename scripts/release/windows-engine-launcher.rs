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

fn configured_private_paths(pack: &Path, bin: &Path) -> Vec<PathBuf> {
    let mut values = vec![bin.to_path_buf()];
    let config = pack.join("engine-runtime-paths.txt");
    if let Ok(text) = fs::read_to_string(config) {
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
            let path = pack.join(line.replace('/', "\\"));
            if path.is_dir() && !values.contains(&path) {
                values.push(path);
            }
        }
    }
    values
}

fn set_private_path(entries: &[PathBuf]) {
    let mut values: Vec<PathBuf> = entries.iter().filter(|p| p.is_dir()).cloned().collect();
    if let Some(root) = env::var_os("SystemRoot").map(PathBuf::from) {
        values.push(root.join("System32"));
        values.push(root);
    }
    if let Ok(joined) = env::join_paths(values) {
        env::set_var("PATH", joined);
    }
}

fn first_prefixed_dir(parent: &Path, prefix: &str) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(parent).ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| path.file_name().and_then(OsStr::to_str).map(|name| name.starts_with(prefix)).unwrap_or(false))
        .collect();
    found.sort();
    found.into_iter().next()
}

fn first_named_dir_below(root: &Path, wanted: &str, depth: usize) -> Option<PathBuf> {
    if depth == 0 || !root.is_dir() { return None; }
    let mut entries: Vec<PathBuf> = fs::read_dir(root).ok()?.filter_map(Result::ok).map(|entry| entry.path()).filter(|path| path.is_dir()).collect();
    entries.sort();
    for path in &entries {
        if path.file_name().and_then(OsStr::to_str) == Some(wanted) { return Some(path.clone()); }
    }
    for path in entries {
        if let Some(found) = first_named_dir_below(&path, wanted, depth - 1) { return Some(found); }
    }
    None
}

fn configure_engine_environment(runtime: &Path) {
    env::remove_var("CONDA_PREFIX");
    env::remove_var("CONDA_DEFAULT_ENV");
    env::remove_var("CONDA_EXE");
    env::remove_var("MAMBA_EXE");
    env::remove_var("PYTHONPATH");
    env::set_var("PYTHONHOME", runtime);
    env::set_var("PYTHONNOUSERSITE", "1");

    for tess in [
        runtime.join("share").join("tessdata"),
        runtime.join("Library").join("share").join("tessdata"),
    ] {
        if tess.is_dir() {
            env::set_var("TESSDATA_PREFIX", tess);
            break;
        }
    }

    env::set_var("MAGICK_HOME", runtime);
    for parent in [runtime.join("etc"), runtime.join("share"), runtime.join("Library").join("etc"), runtime.join("Library").join("share")] {
        if let Some(path) = first_prefixed_dir(&parent, "ImageMagick-") {
            env::set_var("MAGICK_CONFIGURE_PATH", path);
            break;
        }
    }
    for parent in [runtime.join("lib"), runtime.join("Library").join("lib")] {
        if let Some(coders) = first_named_dir_below(&parent, "coders", 4) {
            env::set_var("MAGICK_CODER_MODULE_PATH", coders);
            break;
        }
    }

    let mut ghost_paths: Vec<PathBuf> = Vec::new();
    for ghost_root in [runtime.join("share").join("ghostscript"), runtime.join("Library").join("share").join("ghostscript")] {
        if let Ok(entries) = fs::read_dir(&ghost_root) {
            let mut versions: Vec<PathBuf> = entries.filter_map(Result::ok).map(|entry| entry.path()).filter(|path| path.is_dir()).collect();
            versions.sort();
            if let Some(version) = versions.into_iter().last() {
                for candidate in [version.join("Resource").join("Init"), version.join("lib")] {
                    if candidate.is_dir() { ghost_paths.push(candidate); }
                }
            }
        }
    }
    if let Ok(joined) = env::join_paths(ghost_paths) {
        env::set_var("GS_LIB", joined);
    }
}

fn main() -> ExitCode {
    let me = match env::current_exe() {
        Ok(v) => v,
        Err(e) => { eprintln!("FileFlow engine launcher: current_exe: {e}"); return ExitCode::from(111); }
    };
    let bin = match me.parent() {
        Some(v) => v,
        None => return ExitCode::from(112),
    };
    let pack = match bin.parent() {
        Some(v) => v,
        None => return ExitCode::from(113),
    };
    let stem = me.file_stem().and_then(OsStr::to_str).unwrap_or("");
    let spec = bin.join(format!("{stem}.target"));
    let text = match fs::read_to_string(&spec) {
        Ok(v) => v,
        Err(e) => { eprintln!("FileFlow engine launcher: {}: {e}", spec.display()); return ExitCode::from(114); }
    };
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let target_line = match lines.next() {
        Some(v) => v.trim(),
        None => return ExitCode::from(115),
    };
    let target = if let Some(rest) = target_line.strip_prefix("{PACK}/") {
        pack.join(rest.replace('/', "\\"))
    } else {
        PathBuf::from(target_line)
    };

    let runtime = pack.join("share").join("runtime");
    let private_paths = configured_private_paths(pack, bin);
    set_private_path(&private_paths);
    configure_engine_environment(&runtime);

    let mut command = Command::new(&target);
    for line in lines {
        command.arg(replace_pack(line.trim(), pack));
    }
    command.args(env::args_os().skip(1));

    match command.status() {
        Ok(status) => ExitCode::from(status.code().unwrap_or(1) as u8),
        Err(e) => {
            eprintln!("FileFlow engine launcher: {}: {e}", target.display());
            ExitCode::from(116)
        }
    }
}
