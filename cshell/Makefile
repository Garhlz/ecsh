CC = gcc
CFLAGS = -Wall -Wextra -std=c11

TARGET = ecsh

all: $(TARGET)

$(TARGET): shell.c
	$(CC) $(CFLAGS) -o $(TARGET) shell.c

clean:
	rm -f $(TARGET)

.PHONY: all clean
