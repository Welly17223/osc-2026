RISCV_GNU ?= riscv64-elf
CC = $(RISCV_GNU)-gcc
LD = $(RISCV_GNU)-ld
OBJCOPY = $(RISCV_GNU)-objcopy
CFLAGS = -mcmodel=medany -ffreestanding -nostdlib -g -Wall
QEMU ?= qemu-system-riscv64
TARGET = kernel

.PHONY: build run img clean

build: clean
	$(CC) $(CFLAGS) -c *.S *.c
	$(LD) -T link.ld -o $(TARGET).elf *.o
	$(OBJCOPY) -O binary $(TARGET).elf $(TARGET)

run: build $(TARGET)
	$(QEMU) -M virt -m 8G -kernel $(TARGET) -display none -serial stdio

img: CFLAGS+=-DORANGE_PI
img: build $(TARGET)
	mkimage -f kernel.its kernel.fit

clean:
	rm -f $(TARGET) $(TARGET).elf *.o *.fit
