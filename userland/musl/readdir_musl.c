/* Static musl dir smoke: opendir/readdir on "." and print names. */
#include <stdio.h>
#include <dirent.h>
#include <string.h>
#include <errno.h>

int main(void) {
    DIR *d = opendir(".");
    if (!d) {
        d = opendir("/");
    }
    if (!d) {
        printf("opendir failed errno=%d\n", errno);
        return 1;
    }
    int n = 0;
    int saw_bin = 0;
    int saw_hello = 0;
    struct dirent *e;
    while ((e = readdir(d)) != NULL) {
        printf("ent: %s\n", e->d_name);
        n++;
        if (strcmp(e->d_name, "bin") == 0)
            saw_bin = 1;
        if (strcmp(e->d_name, "hello.txt") == 0)
            saw_hello = 1;
    }
    closedir(d);
    printf("count=%d bin=%d hello.txt=%d\n", n, saw_bin, saw_hello);
    if (n < 2) {
        printf("readdir_musl: BAD (too few entries)\n");
        return 2;
    }
    printf("readdir_musl: OK\n");
    return 0;
}
