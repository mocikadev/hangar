use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=data/resources.xml");
    println!("cargo:rerun-if-changed=../../resources/shared/hangar-icon.svg");
    let target = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo provides OUT_DIR"))
        .join("hangar.gresource");
    let status = Command::new("glib-compile-resources")
        .args([
            "data/resources.xml",
            "--sourcedir=../../resources/shared",
            "--target",
        ])
        .arg(target)
        .status()
        .expect("install libglib2.0-dev-bin (glib-compile-resources)");
    assert!(status.success(), "could not compile native icon resources");
}
