typedef void (*task_callback_t)(void *arg);

extern void free(void *);
extern void *allocate(unsigned long);
extern void uart_puts(char *);
extern void uart_putc(char);
extern void uart_hex(long);
extern void uart_dec(long);
extern void add_task(task_callback_t callback, void *arg, int priority);
extern void add_timer(task_callback_t, void*, unsigned long);

#define MAX_ORDER (0x14LU)
#define PAGE_SIZE (0x1000LU)
#define MAX_ALLOC_SIZE 0x1f6b65000
#define NULL 0

struct test_arg {
  int msg1, msg2;
};

// An example use case
void test_task_cb(void *arg) {
  uart_puts("[Task] Executing Priority ");
  uart_puts((char *)arg);
  uart_puts("\n");
}

void test_task_cb1(void *_arg) {
  struct test_arg *arg = _arg;
  uart_puts("msg1: ");
  uart_hex(arg->msg1);
  uart_puts(" msg2 ");
  uart_hex(arg->msg2);
  uart_puts("\n");
  free(arg);
}

void empty(void* _arg) {}

int priority_set[4];

void p1_callback(void *_args){
    uart_puts("P1 start\n");
    uart_puts("P1 end\n");
}

void p3_callback(void *_args){
    uart_puts("P3 start\n");
    add_task(p1_callback, NULL, priority_set[0]);
    add_timer(NULL, NULL, 0);
    uart_puts("P3 end\n");
}

void p2_callback(void *_args){
    uart_puts("P2 start\n");
    add_task(p3_callback, NULL, priority_set[2]);
    add_timer(NULL, NULL, 0);
    uart_puts("P2 end\n");
}

void p4_callback(void *_args){
    uart_puts("P4 start\n");
    add_task(p2_callback, NULL, priority_set[1]);
    add_timer(NULL, NULL, 0);
    uart_puts("P4 end\n");
}

void test_func(){
    int from_small_to_big = 0; // set to 0 if the task with a smaller number has a higher priority
    if(from_small_to_big){
        priority_set[0] = 10;
        priority_set[1] = 20;
        priority_set[2] = 30;
        priority_set[3] = 40;
    }else{
        priority_set[0] = 40;
        priority_set[1] = 30;
        priority_set[2] = 20;
        priority_set[3] = 10;
    }

    add_task(p4_callback, NULL, priority_set[3]);
}


void test_addtask() {
  struct test_arg *arg = (struct test_arg *)allocate(sizeof(struct test_arg));
  *arg = (struct test_arg){
      .msg1 = 1,
      .msg2 = 2,
  };
  add_task(test_task_cb1, arg, 7);
  add_task(test_task_cb, "3", 3);
  add_task(test_task_cb, "2", 2);
  add_task(test_task_cb, "1", 1);
  add_task(test_task_cb, "4", 4);
  add_task(test_task_cb, "5", 5);
  add_task(test_task_cb, "6", 6);
  add_task(test_task_cb, "6", 6);
}

void test_alloc_1() {
  /***************** Case 2 *****************/

  uart_puts("\n===== Part 1 =====\n");

  void *p1 = allocate(129);
  free(p1);

  uart_puts("\n=== Part 1 End ===\n");

  uart_puts("\n===== Part 2 =====\n");

  // Allocate all blocks at order 0, 1, 2 and 3
  int NUM_BLOCKS_AT_ORDER_0 = 2; // Need modified
  int NUM_BLOCKS_AT_ORDER_1 = 4;
  int NUM_BLOCKS_AT_ORDER_2 = 3;
  int NUM_BLOCKS_AT_ORDER_3 = 3;

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
