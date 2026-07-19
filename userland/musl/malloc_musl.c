#include <stdio.h>
#include <stdlib.h>
#include <string.h>
int main(void) {
    char *p = malloc(64);
    if (!p) {
        printf("malloc failed\n");
        return 1;
    }
    strcpy(p, "heap ok");
    printf("malloc: %s len=%d\n", p, (int)strlen(p));
    free(p);
    void *q = malloc(4096);
    if (!q) {
        printf("big malloc failed\n");
        return 2;
    }
    memset(q, 0xA5, 4096);
    printf("malloc4k: ok\n");
    free(q);
    return 0;
}
