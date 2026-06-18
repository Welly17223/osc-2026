#include <errno.h>
#include <fcntl.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <termios.h>
#include <unistd.h>

const uint32_t MAGIC = 0x544F4F42;
const uint32_t BEGIN = 0x123fd8ae;
const uint32_t BEGiN = 0xaed83f12;

int set_serial_attributes(int fd, int speed) {
  struct termios tty;

  // 取得當前的設定
  if (tcgetattr(fd, &tty) != 0) {
    perror("tcgetattr failed");
    return -1;
  }

  // 設定 Baud Rate (請根據你的設備修改，例如 B115200)
  cfsetospeed(&tty, speed);
  cfsetispeed(&tty, speed);

  // 設定 8N1 (8 bits, no parity, 1 stop bit)
  tty.c_cflag &= ~PARENB; // 無同位檢查 (No Parity)
  tty.c_cflag &= ~CSTOPB; // 1 個停止位元 (1 Stop bit)
  tty.c_cflag &= ~CSIZE;  // 清除資料位元遮罩
  tty.c_cflag |= CS8;     // 8 個資料位元 (8 Data bits)

  // 關閉硬體流控 (Hardware flow control) - 這是導致 macOS 寫入卡住的主因！
  tty.c_cflag &= ~CRTSCTS;

  // 開啟接收器，忽略數據機控制線
  tty.c_cflag |= CREAD | CLOCAL;

  // 關閉軟體流控 (Software flow control)
  tty.c_iflag &= ~(IXON | IXOFF | IXANY);

  // 進入 Raw Mode (關閉特殊字元處理、Echo 等)
  tty.c_lflag &= ~(ICANON | ECHO | ECHOE | ISIG);
  tty.c_oflag &= ~OPOST;

  // 關閉回車換行符號的自動轉換
  tty.c_iflag &= ~(IGNBRK | BRKINT | PARMRK | ISTRIP | INLCR | IGNCR | ICRNL);

  // 設定讀取行為：此處設定為 Block 直到讀到最少 1 個字元，或是等待 0.5 秒
  tty.c_cc[VMIN] = 1;
  tty.c_cc[VTIME] = 5; // 0.5 秒 (單位是 0.1 秒)

  // 將設定寫回設備
  if (tcsetattr(fd, TCSANOW, &tty) != 0) {
    perror("tcsetattr failed");
    return -1;
  }
  return 0;
}

int main(int argc, char *argv[]) {
  int dev_fd = open(argv[1], O_RDWR | O_NOCTTY | O_NDELAY);
  if (dev_fd < 0) {
    perror("Open dev");
    return 1;
  }
  fcntl(dev_fd, F_SETFL, 0);
  if (set_serial_attributes(dev_fd, B115200) < 0) {
    close(dev_fd);
    return 1;
  }

  FILE *kernel = fopen(argv[2], "rb");
  if (kernel == NULL) {
    close(dev_fd);
    perror("Open kernel");
    return 1;
  }

  fseek(kernel, 0, SEEK_END);
  const uint32_t file_size = ftell(kernel);
  fprintf(stderr, "File size: %u\n", file_size);
  long ret = 0;
  char loadw[] = "load\n";
  write(dev_fd, loadw, 5);

  const uint8_t *magic_ptr = (uint8_t *)&MAGIC;
  ret = write(dev_fd, magic_ptr, 4);
  if (ret < 0) {
    perror("Write dev");
    close(dev_fd);
    fclose(kernel);
    return 1;
  }

  write(dev_fd, (uint8_t *)&file_size, 4);

  fseek(kernel, 0, SEEK_SET);
  uint8_t *buf = malloc(file_size);
  if (buf == NULL)
    exit(1);

  size_t read_size = fread(buf, 1, file_size, kernel);

  if (read_size != file_size) {
    fprintf(stderr, "read_size %lu no reach file size %u\n", read_size,
            file_size);
    exit(1);
  }

  printf("0x%x\n", *(uint32_t *)(buf + (0x74 << 2)));

  FILE *copy = fopen("./kernel.bak.bin", "wb");
  int write_num = 0;
  const int block_size = 1024;
  char ack_buf[128];
  int ack_buf_pos = 0;

  while (1) {
    int nbyte = read(dev_fd, ack_buf + ack_buf_pos, 127 - ack_buf_pos);

    if (nbyte < 0) {
      perror("Read");
      // return 1;
    }

    ack_buf[nbyte + ack_buf_pos] = 0;

    if (strchr(ack_buf, '\n') == NULL) {
      ack_buf_pos += nbyte;
      continue;
    } else {
      ack_buf_pos = 0;
      printf("Recv: %s\n", ack_buf);
    }

    if (strstr(ack_buf, "start") != NULL)
      break;
  }

  printf("start to transmit\n");
  bool resend = 0;
  while (write_num < read_size) {
    int current_write_size = read_size - write_num >= block_size
                                 ? block_size
                                 : read_size - write_num;
    if (resend == 0)
      write(dev_fd, (uint8_t *)&BEGiN, sizeof(BEGIN));
    else 
      resend = 0;

    ack_buf_pos = 0;
    while (1) {
      int nbyte = read(dev_fd, ack_buf + ack_buf_pos, 127 - ack_buf_pos);
      if (nbyte < 0) {
        return 1;
      }
      ack_buf[nbyte + ack_buf_pos] = 0;

      if (strchr(ack_buf, '\n') != NULL) {
        break;
      } else {
        ack_buf_pos += nbyte;
      }
      // sleep(1);
      // write(dev_fd, (uint8_t *)&BEGiN, sizeof(BEGIN));
      // printf("resend ack\n");
    }
    // printf("Recv: %s\n", ack_buf);

    write(dev_fd, buf + write_num, current_write_size);
    ack_buf_pos = 0;
    while (1) {
      int nbyte = read(dev_fd, ack_buf + ack_buf_pos, 127 - ack_buf_pos);
      if (nbyte < 0) {
        return 1;
      }
      ack_buf[nbyte + ack_buf_pos] = 0;

      if (strchr(ack_buf, '\n') != NULL) {
        break;
      } else {
        ack_buf_pos += nbyte;
      }
    }

    if (strncmp(ack_buf, "ACK", 3) == 0) {
      write_num += current_write_size;
      printf("send %9d/%-9d\r", write_num, file_size);
      fflush(stdout);
    } else if (strncmp(ack_buf, "NAK", 3) == 0) {
      resend = 1;
      sleep(1);
    }
  }

  printf("\nWrite %d\n", write_num);
  fclose(copy);

  free(buf);
  close(dev_fd);
  fclose(kernel);
  return 0;
}
