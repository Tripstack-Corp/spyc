#include <string.h>
#include <stdio.h>
int main(int argc, char **argv) {
    char buf[256];
    memset(buf, argc, (size_t)argc * 100);
    printf("%d\n", buf[7]);
    return 0;
}
