use std::env;

fn main() {
    let mia_dir = format!("{}/mia", env::var("OUT_DIR").unwrap());
    eprintln!("mia_dir={}", &mia_dir);

    println!("cargo::rerun-if-changed={}", mia_dir);

    let mia_version = "v0.1.0";
    if std::path::Path::new(&mia_dir).exists() {
        eprintln!("pull&checkout {}", &mia_version);
        std::process::Command::new("git")
            .args(["-C", &mia_dir, "pull", "--depth", "1"])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["-C", &mia_dir, "checkout", mia_version])
            .status()
            .unwrap();
    } else {
        eprintln!("clone {}", &mia_version);
        let mia_url = "https://github.com/gevulotnetwork/mia.git";
        std::process::Command::new("git")
            .args([
                "clone",
                "--depth",
                "1",
                "--branch",
                mia_version,
                mia_url,
                &mia_dir,
            ])
            .status()
            .unwrap();
    }

    let out_dir = env::var("OUT_DIR").unwrap();
    eprintln!("out_dir={}", &out_dir);

    // Filter all CARGO* env vars.
    // NOTE: This is required because MIA has its own Cargo config, which
    // will conflict with env and will not be used. For simplicity,
    // we just clear all Cargo-related variables to emulate the process
    // of normal MIA build like `cd components/mia && cargo b -r`
    let filtered_env = std::env::vars().filter(|(var, _)| !var.starts_with("CARGO"));

    env::set_current_dir(&mia_dir).unwrap();
    // TODO: we could use --out-dir here to avoid relying on mias target
    // directory structure in the future, but its still unstable.
    let status = std::process::Command::new("cargo")
        .arg("build")
        .arg("--release")
        .arg("--target-dir")
        .arg(&out_dir)
        .env_clear()
        .envs(filtered_env)
        .status()
        .unwrap();
    assert!(status.success());
}
