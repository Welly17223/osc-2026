fn main() {
    cc::Build::new()
        .file("src/test_memory.c")
        .target("riscv64gc-unknown-none-elf")
        .compiler("riscv64-elf-gcc")
        .flag("-mcmodel=medany")
        .pic(true)
        .compile("test_memory");
    println!("cargo:rerun-if-changed=test_c/src/test_memory.c");
    println!("cargo:rerun-if-changed=src/test_memory.c");
}
