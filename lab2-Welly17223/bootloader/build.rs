fn main() {
    println!("cargo:rustc-link-arg-bin=bootloader=-T./bootloader/link_script.ld");
    cc::Build::new()
        .file("src/start.s")
        .target("riscv64gc-unknown-none-elf")
        .compiler("riscv64-elf-gcc")
        .compile("start");
    println!("cargo:return-if-changed=./bootloader/src/start.s")
}
