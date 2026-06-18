TARGET_DIR=target/riscv64imac-unknown-none-elf/debug/
CARGO_FLAGS=

all: kernel bootloader

run: kernel
	qemu-system-riscv64 -M virt -kernel ${TARGET_DIR}/kernel.bin -display none -serial stdio

bootloader:
	cargo build $(CARGO_FLAGS) -p bootloader
	riscv64-elf-objcopy -O binary ${TARGET_DIR}/bootloader ${TARGET_DIR}/bootloader-kernel.bin

kernel:
	cargo build $(CARGO_FLAGS)
	riscv64-elf-objcopy -O binary ${TARGET_DIR}/os ${TARGET_DIR}/kernel.bin

kernel-pi: CARGO_FLAGS+=--no-default-features --features orangePI
kernel-pi:
	cargo build $(CARGO_FLAGS)
	riscv64-elf-objcopy -O binary ${TARGET_DIR}/os ${TARGET_DIR}/kernel.bin

debug: kernel
	qemu-system-riscv64 -M virt -kernel ${TARGET_DIR}/kernel.bin -display none -serial stdio -S -s

boot: bootloader
	qemu-system-riscv64 -M virt -initrd initramfs.cpio -kernel ${TARGET_DIR}/bootloader-kernel.bin -display none -serial stdio

img: CARGO_FLAGS+=--no-default-features --features orangePI
img: bootloader
	mkimage -f kernel-bootloader.its kernel.fit

clean:
	@cargo clean

.PHONY: kernel run all clean debug bootloader
