fn main() {
    let path = "../../../VERSION";
    println!("cargo:rerun-if-changed={path}");
    let version = std::fs::read_to_string(path)
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|_| "dev".into());
    println!("cargo:rustc-env=DETOUR_VERSION={version}");
}
