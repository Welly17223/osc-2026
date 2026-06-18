fn main() {
    cc::Build::new()
        .files(["src/sbi.c", "src/interrupt/handle_exception.S", "src/context_switch.S"])
        .target("riscv64gc-unknown-none-elf") // 指定目標架構
        .compiler("riscv64-elf-gcc") // 指定你的編譯器
        .compile("sbi"); // 將編譯結果命名為 libuart.a
    println!("cargo:rerun-if-changed=linker.ld");
    println!("cargo:rerun-if-changed=src/sbi.c");
    println!("cargo:rerun-if-changed=src/start.s");
    println!("cargo:rerun-if-changed=src/context_switch.S");
    println!("cargo:rerun-if-changed=src/interrupt/handle_exception.S");
}
