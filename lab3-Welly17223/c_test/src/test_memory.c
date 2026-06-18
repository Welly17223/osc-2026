extern void free(void *);
extern void *allocate(unsigned long);
extern void uart_puts(char *);

#define MAX_ORDER (0x14LU)
#define PAGE_SIZE (0x1000LU)
#define MAX_ALLOC_SIZE 0x1f6b65000

void test_alloc_1() {
  /***************** Case 2 *****************/

  uart_puts("\n===== Part 1 =====\n");

  void *p1 = allocate(129);
  free(p1);

  uart_puts("\n=== Part 1 End ===\n");

  uart_puts("\n===== Part 2 =====\n");

  // Allocate all blocks at order 0, 1, 2 and 3
  int NUM_BLOCKS_AT_ORDER_0 = 3; // Need modified
  int NUM_BLOCKS_AT_ORDER_1 = 5;
  int NUM_BLOCKS_AT_ORDER_2 = 5;
  int NUM_BLOCKS_AT_ORDER_3 = 5;

  void *ps0[NUM_BLOCKS_AT_ORDER_0];
  void *ps1[NUM_BLOCKS_AT_ORDER_1];
  void *ps2[NUM_BLOCKS_AT_ORDER_2];
  void *ps3[NUM_BLOCKS_AT_ORDER_3];
  for (int i = 0; i < NUM_BLOCKS_AT_ORDER_0; ++i) {
    ps0[i] = allocate(4096);
  }
  for (int i = 0; i < NUM_BLOCKS_AT_ORDER_1; ++i) {
    ps1[i] = allocate(8192);
  }
  for (int i = 0; i < NUM_BLOCKS_AT_ORDER_2; ++i) {
    ps2[i] = allocate(16384);
  }
  for (int i = 0; i < NUM_BLOCKS_AT_ORDER_3; ++i) {
    ps3[i] = allocate(32768);
  }

  uart_puts("\n-----------\n");

  long MAX_BLOCK_SIZE = PAGE_SIZE * (1 << MAX_ORDER);

  /* **DO NOT** uncomment this section */
  void *c1, *c2, *c3, *c4, *c5, *c6, *c7, *c8, *p2, *p3, *p4, *p5, *p6, *p7;

  p1 = allocate(4095);
  free(p1); // 4095
  p1 = allocate(4095);

  c1 = allocate(1000);
  c2 = allocate(1023);
  c3 = allocate(999);
  c4 = allocate(1010);
  free(c3); // 999
  c5 = allocate(989);
  c3 = allocate(88);
  c6 = allocate(1001);
  free(c3); // 88
  c7 = allocate(2045);
  c8 = allocate(1);

  p2 = allocate(4096);
  free(c8); // 1
  p3 = allocate(16000);
  free(p1); // 4095
  free(c7); // 2045
  p4 = allocate(4097);
  p5 = allocate(MAX_BLOCK_SIZE + 1);
  p6 = allocate(MAX_BLOCK_SIZE);
  free(p2); // 4096
  free(p4); // 4097
  p7 = allocate(7197);

  free(p6); // MAX_BLOCK_SIZE
  free(p3); // 16000
  free(p7); // 7197
  free(c1); // 1000
  free(c6); // 1001
  free(c2); // 1023
  free(c5); // 989
  free(c4); // 1010

  uart_puts("\n-----------\n");

  // Free all blocks remaining
  for (int i = 0; i < NUM_BLOCKS_AT_ORDER_0; ++i) {
    free(ps0[i]);
  }
  for (int i = 0; i < NUM_BLOCKS_AT_ORDER_1; ++i) {
    free(ps1[i]);
  }
  for (int i = 0; i < NUM_BLOCKS_AT_ORDER_2; ++i) {
    free(ps2[i]);
  }
  for (int i = 0; i < NUM_BLOCKS_AT_ORDER_3; ++i) {
    free(ps3[i]);
  }

  uart_puts("\n=== Part 2 End ===\n");
}
