fn main() {
    println!("cargo:rustc-link-arg-bin=bootloader=-T./bootloader/link_script.ld");
    cc::Build::new()
        .file("src/start.s")
        .target("riscv64gc-unknown-none-elf")
        .compiler("riscv64-elf-gcc")
        .flags([
            "-O2",
            "-march=rv64gc_zba_zbb_zbc_zbs_zicbom_zicboz_zicsr_zifencei",
            "-mabi=lp64d",
            "-ffreestanding",
        ])
        .compile("start");
    println!("cargo:return-if-changed=./bootloader/src/start.s");
    println!("cargo:return-if-changed=./src/start.s");
}
