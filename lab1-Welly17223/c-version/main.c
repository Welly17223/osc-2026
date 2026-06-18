extern char uart_getc(void);
extern void uart_putc(char c);
extern void uart_puts(const char *s);
extern void uart_hex(unsigned long h);

#define SBI_EXT_SET_TIMER 0x0
#define SBI_EXT_SHUTDOWN 0x8
#define SBI_EXT_BASE 0x10

#define min(a, b)                                                              \
  ({                                                                           \
    typeof(a) _a = a;                                                          \
    typeof(b) _b = b;                                                          \
    _a > _b ? _a : _b;                                                         \
  })

#define KEY_UP "\x1b[A"
#define KEY_CLEAR "\x1b[2K"
#define KEY_DOWN "\x1b[B"

enum sbi_ext_base_fid {
  SBI_EXT_BASE_GET_SPEC_VERSION,
  SBI_EXT_BASE_GET_IMP_ID,
  SBI_EXT_BASE_GET_IMP_VERSION,
  SBI_EXT_BASE_PROBE_EXT,
  SBI_EXT_BASE_GET_MVENDORID,
  SBI_EXT_BASE_GET_MARCHID,
  SBI_EXT_BASE_GET_MIMPID,
};

struct sbiret {
  long error;
  long value;
};

struct sbiret sbi_ecall(int ext, int fid, unsigned long arg0,
                        unsigned long arg1, unsigned long arg2,
                        unsigned long arg3, unsigned long arg4,
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

int strcmp(const char *s1, const char *s2) {
  int cmp = *s1 - *s2;
  while (cmp == 0 && *s1 != '\0' && *s2 != '\0') {
    s1++;
    s2++;
    cmp = *s1 - *s2;
  }
  return cmp;
}

/**
 * sbi_get_spec_version() - Get the SBI specification version.
 *
 * Return: The current SBI specification version.
 * The minor number of the SBI specification is encoded in the low 24 bits,
 * with the major number encoded in the next 7 bits. Bit 31 must be 0.
 */
long sbi_get_spec_version(void) {
  // TODO: Implement this function
  struct sbiret ret =
      sbi_ecall(SBI_EXT_BASE, SBI_EXT_BASE_GET_SPEC_VERSION, 0, 0, 0, 0, 0, 0);

  if (ret.error != 0) {
    uart_puts("error happen here: ");
    uart_hex(-ret.error);
    uart_putc('\n');
  }

  return ret.value;
}

/**
 * sbi_probe_extension() - Check if an SBI extension ID is supported or not.
 * @extid: The extension ID to be probed.
 *
 * Return: 1 or an extension specific nonzero value if yes, 0 otherwise.
 */
long sbi_probe_extension(int extid) {
  // TODO: Implement this function
  struct sbiret ret =
      sbi_ecall(SBI_EXT_BASE, SBI_EXT_BASE_PROBE_EXT, extid, 0, 0, 0, 0, 0);

  if (ret.error != 0) {
    uart_puts("error happen here: ");
    uart_hex(-ret.error);
    uart_putc('\n');
  }

  return ret.value;
}

long sbi_get_impl_id() {
  struct sbiret ret =
      sbi_ecall(SBI_EXT_BASE, SBI_EXT_BASE_GET_IMP_ID, 0, 0, 0, 0, 0, 0);
  if (ret.error != 0) {
    uart_puts("error happen here: ");
    uart_hex(-ret.error);
    uart_putc('\n');
  }

  return ret.value;
}
long sbi_get_impl_version() {
  struct sbiret ret =
      sbi_ecall(SBI_EXT_BASE, SBI_EXT_BASE_GET_IMP_VERSION, 0, 0, 0, 0, 0, 0);
  if (ret.error != 0) {
    uart_puts("error happen here: ");
    uart_hex(-ret.error);
    uart_putc('\n');
  }

  return ret.value;
}

void sbi_shutdown() { sbi_ecall(0x53525354, 0, 0, 0, 0, 0, 0, 0); }

void new_command() { uart_puts("\n> "); }

void start_kernel() {
  uart_puts("\n112550172 kernel starting...\n");

  while (1) {
    new_command();
    char buf[32], *s = buf, *buf_end = buf + sizeof(buf);
    unsigned int is_full = 0;
    for (*s = '\0';; s++) {
      long ch = uart_getc();

      switch (ch) {
      // backspace
      case 0x08:
      // Delete
      case 0x7f:
        if (is_full & 0x1) {
          uart_puts(KEY_DOWN KEY_CLEAR KEY_UP);
          is_full &= ~(0x1);
        }
        if (s - buf > 0) {
          *(s - 1) = '\0';
          s -= 2;
          uart_puts("\b \b");
        } else {
          s--;
        }

        break;

      case '\n':
        goto end_loop;

      // EOF ^D
      case 0x04:
        goto main_end;
      default:
        if (s != buf_end) {
          *s = ch;
          *(s + 1) = '\0';
          uart_putc(ch);
        } else {
          *s = '\0';
          s--;
          if (!(is_full & 0x1)) {
            uart_puts("\n\r"
                      "[warn]: buf is full!" KEY_UP KEY_CLEAR "\r> ");
            uart_puts(buf);
          }
          is_full |= 0x1;
        }
      }
    }
  end_loop:
    *s = '\0';

    if (strcmp(buf, "help") == 0) {
      uart_puts("\nAvaliable command:");
      uart_puts("\n  help   - show all commands.");
      uart_puts("\n  hello  - print hello world.");
      uart_puts("\n  info   - print system info.");
      uart_puts("\n  exit   - exit.");

    } else if (strcmp(buf, "hello") == 0) {
      uart_puts("\nHello world!");

    } else if (strcmp(buf, "info") == 0) {
      uart_puts("\nSystem information:\n");
      uart_puts("  OpenSBI specification version: ");
      uart_hex(sbi_get_spec_version());
      uart_puts("\n");

      uart_puts("  Implementation ID: ");
      uart_hex(sbi_get_impl_id());
      uart_puts("\n");

      uart_puts("  Implementation version:  ");
      uart_hex(sbi_get_impl_version());

    } else if (strcmp(buf, "exit") == 0) {
      goto main_end;
    } else {
      uart_puts("\n[warn]: Unknown command '");
      uart_puts(buf);
      uart_putc('\'');
    }
  }

main_end:
  uart_puts("\nBye!\n");
  sbi_shutdown();
}
