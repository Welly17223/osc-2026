TARGET_DIR=target/riscv64imac-unknown-none-elf/debug/
CARGO_FLAGS=

all: build

run: build
	qemu-system-riscv64 -M virt -kernel ${TARGET_DIR}/kernel.bin -display none -serial stdio

build:
	cargo build $(CARGO_FLAGS)
	riscv64-elf-objcopy -O binary ${TARGET_DIR}/rust-version ${TARGET_DIR}/kernel.bin

debug: build
	qemu-system-riscv64 -M virt -kernel ${TARGET_DIR}/kernel.bin -display none -serial stdio -S -s

img: CARGO_FLAGS+=--no-default-features --features orangePI
img: build
	mkimage -f kernel.its kernel.fit

clean:
	@cargo clean

.PHONY: build run all clean debug
