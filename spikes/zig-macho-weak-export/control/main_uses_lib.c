#include <string.h>
#include <stdio.h>
unsigned strong_hidden(unsigned);
int main(int argc, char **argv) {
    char buf[256];
    memset(buf, argc, (size_t)argc * 100);
    printf("%u %d\n", strong_hidden((unsigned)argc), buf[7]);
    return 0;
}
