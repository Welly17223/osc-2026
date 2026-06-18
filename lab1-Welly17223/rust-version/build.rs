fn main() {
    println!("cargo:rustc-link-arg-bin=rust-version=-Tlink_script.ld");
    cc::Build::new()
        .file("src/sbi.c")
        .target("riscv64gc-unknown-none-elf") // 指定目標架構
        .compiler("riscv64-elf-gcc") // 指定你的編譯器
        .compile("sbi"); // 將編譯結果命名為 libuart.a
    println!("cargo:rerun-if-changed=linker.ld");
    println!("cargo:rerun-if-changed=src/sbi.c");
    println!("cargo:rerun-if-changed=src/start.s");
}
