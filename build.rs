fn main() {
    println!("cargo:rerun-if-changed=assets/app-icon.rc");
    println!("cargo:rerun-if-changed=assets/app-icon.ico");

    #[cfg(windows)]
    embed_resource::compile("assets/app-icon.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("compile Windows application icon");
}
