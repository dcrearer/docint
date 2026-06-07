fn main() {
    // Tell Cargo to rerun this build script if git HEAD changes
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/heads/main");

    built::write_built_file().expect("Failed to acquire build-time information");
}
