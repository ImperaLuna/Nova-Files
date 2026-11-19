use std::path::Path;
use std::process::Command as Process;

pub fn run_again_in(root: &Path, root_var: &str, name: &str) {
    let data = root.join("data");
    let config = root.join("config");
    let status = Process::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--test-threads=1"])
        .env(root_var, root)
        .env("HOME", root)
        .env("XDG_DATA_HOME", &data)
        .env("XDG_DATA_DIRS", &data)
        .env("XDG_CONFIG_HOME", &config)
        .env("XDG_CONFIG_DIRS", &config)
        .env_remove("XDG_CURRENT_DESKTOP")
        .status()
        .unwrap();
    std::fs::remove_dir_all(root).unwrap();
    assert!(status.success());
}
