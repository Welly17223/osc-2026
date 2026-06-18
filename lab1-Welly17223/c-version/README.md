# Exercise 1-2

## Intro.
In the previous exercise, you accessed the UART hardware directly by writing to specific memory addresses (MMIO). However, in a modern RISC-V system, the OS kernel (Supervisor Mode) often needs to communicate with the lower-level firmware (Machine Mode) to perform privileged operations like timer setup, system shutdown, or remote fence. This standard interface is called the **Supervisor Binary Interface (SBI)**.

In this exercise, you will implement wrapper functions to interact with the SBI Base Extension.

## SBI Calling Convention
Just like standard function calls use registers to pass arguments, SBI calls follow a specific convention using the `ecall` instruction. When the kernel executes ecall, the CPU traps into Machine Mode (OpenSBI), which handles the request.


| Register | Usage | Description |
| -------- | -------- | -------- |
| a7 | Extension ID (EID) | Identifies which SBI module to call (e.g., Base, Timer, HSM). |
| a6 |	Function ID (FID) |	Identifies the specific function within that extension. |
| a0 - a5 |	Arguments | Parameters passed to the SBI function. |
| a0| Error Code | Return value: Error code (0 for success). |
| a1| Return Value | Return value: The result of the operation. |

We have provided a low-level helper function, `sbi_ecall`, which handles the register setup and the ecall instruction for you. Your task is to use this helper to implement specific SBI functionalities.

```c
struct sbiret {
    long error;
    long value;
};

struct sbiret sbi_ecall(int ext,
                        int fid,
                        unsigned long arg0,
                        unsigned long arg1,
                        unsigned long arg2,
                        unsigned long arg3,
                        unsigned long arg4,
                        unsigned long arg5) {
    struct sbiret ret;
    register unsigned long a0 asm("a0") = (unsigned long)arg0;
    register unsigned long a1 asm("a1") = (unsigned long)arg1;
    register unsigned long a2 asm("a2") = (unsigned long)arg2;
    register unsigned long a3 asm("a3") = (unsigned long)arg3;
    register unsigned long a4 asm("a4") = (unsigned long)arg4;
    register unsigned long a5 asm("a5") = (unsigned long)arg5;
    register unsigned long a6 asm("a6") = (unsigned long)fid;
    register unsigned long a7 asm("a7") = (unsigned long)ext;
    asm volatile("ecall"
                 : "+r"(a0), "+r"(a1)
                 : "r"(a2), "r"(a3), "r"(a4), "r"(a5), "r"(a6), "r"(a7)
                 : "memory");
    ret.error = a0;
    ret.value = a1;
    return ret;
}
```

## Base Extension
The SBI specification defines several extensions. The Base Extension (EID `0x10`) is mandatory and allows the kernel to query information about the SBI implementation itself, such as the version and supported features.

You need to implement the following two functions using sbi_ecall.
- `sbi_get_spec_version(void)` : This function retrieves the version of the SBI specification that the firmware conforms to.
  - `Extension ID (EID)` : `SBI_EXT_BASE` (`0x10`)
  - `Function ID (FID)` : `SBI_EXT_BASE_GET_SPEC_VERSION` (`0`)
  - `Arguments` : None (`0`)
  - `Return` : The value is returned in `sbiret.value`
  
- `sbi_probe_extension(int extid)` : This function checks whether a specific SBI extension is supported by the firmware.
  - `Extension ID (EID)` : `SBI_EXT_BASE` (`0x10`)
  - `Function ID (FID)` : `SBI_EXT_BASE_PROBE_EXT` (`3`)
  - `Arguments` :
    - `arg0` : The Extension ID to probe (e.g., `SBI_EXT_SET_TIMER` or `SBI_EXT_SHUTDOWN`).
  - `Return` : 
    - Returns `0` if the extension is **not** supported.
    - Returns a non-zero value (usually `1`) if supported.
```c
/**
 * sbi_get_spec_version() - Get the SBI specification version.
 *
 * Return: The current SBI specification version.
 * The minor number of the SBI specification is encoded in the low 24 bits,
 * with the major number encoded in the next 7 bits. Bit 31 must be 0.
 */
long sbi_get_spec_version(void) {
    // TODO: Implement this function
}

/**
 * sbi_probe_extension() - Check if an SBI extension ID is supported or not.
 * @extid: The extension ID to be probed.
 *
 * Return: 1 or an extension specific nonzero value if yes, 0 otherwise.
 */
long sbi_probe_extension(int extid) {
    // TODO: Implement this function
}
```

## Expected Result
If successful, you should see the SBI version and the probe results printed to the UART console. The output will look similar to this (exact version numbers may vary depending on your QEMU/OpenSBI version):
```
SBI specification version: 0x0000000003000000
Probe Set Timer: 0x0000000000000001
Probe Shutdown:  0x0000000000000001
```