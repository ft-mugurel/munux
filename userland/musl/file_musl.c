/* Static musl file smoke: fopen/fread/printf on ext2 path. */
#include <stdio.h>
#include <string.h>

int main(void) {
    FILE *f = fopen("hello.txt", "r");
    if (!f) {
        /* also try absolute / rooted path style */
        f = fopen("/hello.txt", "r");
    }
    if (!f) {
        printf("fopen failed\n");
        return 1;
    }
    char buf[128];
    size_t n = fread(buf, 1, sizeof(buf) - 1, f);
    buf[n] = '\0';
    fclose(f);
    /* strip trailing newlines for one-line check */
    while (n > 0 && (buf[n - 1] == '\n' || buf[n - 1] == '\r')) {
        buf[--n] = '\0';
    }
    printf("file: %s\n", buf);
    printf("file_musl: OK bytes=%d\n", (int)n);
    return 0;
}
