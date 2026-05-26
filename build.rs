use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let web_dir = manifest_dir.join("src/studio/web");
    if !web_dir.exists() {
        return;
    }

    register_rerun_inputs(&web_dir);
    ensure_tool("node");
    ensure_tool("npm");

    if frontend_is_stale(&web_dir) {
        run(&web_dir, "npm", ["ci"]);
        run(&web_dir, "npm", ["run", "build"]);
    }
}

fn register_rerun_inputs(web_dir: &Path) {
    for relative in [
        "package.json",
        "package-lock.json",
        "tsconfig.json",
        "vite.config.ts",
        "index.html",
    ] {
        let path = web_dir.join(relative);
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    register_dir(&web_dir.join("src"));
}

fn register_dir(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            register_dir(&path);
        } else {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}

fn ensure_tool(tool: &str) {
    match Command::new(tool).arg("--version").output() {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            panic!(
                "wt studio frontend build requires `{tool}` on PATH; `{tool} --version` exited with {status}",
                status = output.status
            );
        }
        Err(err) => {
            panic!("wt studio frontend build requires `{tool}` on PATH: {err}");
        }
    }
}

fn frontend_is_stale(web_dir: &Path) -> bool {
    let marker = web_dir.join("dist/index.html");
    let Ok(marker_time) = modified(&marker) else {
        return true;
    };

    input_paths(web_dir)
        .into_iter()
        .filter_map(|path| modified(&path).ok())
        .any(|time| time > marker_time)
}

fn input_paths(web_dir: &Path) -> Vec<PathBuf> {
    let mut paths = [
        "package.json",
        "package-lock.json",
        "tsconfig.json",
        "vite.config.ts",
        "index.html",
    ]
    .into_iter()
    .map(|relative| web_dir.join(relative))
    .filter(|path| path.exists())
    .collect::<Vec<_>>();
    collect_files(&web_dir.join("src"), &mut paths);
    paths
}

fn collect_files(dir: &Path, paths: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, paths);
        } else {
            paths.push(path);
        }
    }
}

fn modified(path: &Path) -> std::io::Result<SystemTime> {
    fs::metadata(path)?.modified()
}

fn run<I, S>(dir: &Path, program: &str, args: I)
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_owned())
        .collect::<Vec<_>>();
    let rendered_args = args
        .iter()
        .map(|arg| arg.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    let status = Command::new(program)
        .args(&args)
        .current_dir(dir)
        .status()
        .unwrap_or_else(|err| {
            panic!(
                "failed to start `{program} {rendered_args}` for wt studio frontend build: {err}"
            )
        });
    if !status.success() {
        panic!("`{program} {rendered_args}` failed for wt studio frontend build with {status}");
    }
}
