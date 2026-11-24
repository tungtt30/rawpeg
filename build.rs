fn main() {
    println!("cargo:rerun-if-changed=rawpeg.rc");
    println!("cargo:rerun-if-changed=assets/rawpeg.ico");

    #[cfg(windows)]
    embed_resource::compile("rawpeg.rc", embed_resource::NONE)
        .manifest_optional()
        .unwrap();
}
