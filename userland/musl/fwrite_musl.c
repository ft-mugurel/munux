/* Static musl file write smoke: fopen w, fprintf, fclose, then read back. */
#include <stdio.h>
#include <string.h>

int main(void) {
    const char *path = "musl_out.txt";
    FILE *f = fopen(path, "w");
    if (!f) {
        f = fopen("/musl_out.txt", "w");
        path = "/musl_out.txt";
    }
    if (!f) {
        printf("fopen write failed\n");
        return 1;
    }
    int n = fprintf(f, "hello musl write %d\n", 42);
    if (n < 0) {
        printf("fprintf failed\n");
        fclose(f);
        return 2;
    }
    if (fclose(f) != 0) {
        printf("fclose failed\n");
        return 3;
    }

    f = fopen(path, "r");
    if (!f) {
        printf("fopen readback failed\n");
        return 4;
    }
    char buf[128];
    size_t m = fread(buf, 1, sizeof(buf) - 1, f);
    buf[m] = '\0';
    fclose(f);
    while (m > 0 && (buf[m - 1] == '\n' || buf[m - 1] == '\r'))
        buf[--m] = '\0';

    printf("readback: %s\n", buf);
    if (strstr(buf, "hello musl write 42") == NULL) {
        printf("fwrite_musl: BAD content\n");
        return 5;
    }
    printf("fwrite_musl: OK\n");
    return 0;
}
