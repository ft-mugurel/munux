/* P10c: dynamically linked glibc hello (needs /lib64/ld-linux-x86-64.so.2 + libc.so.6). */
#include <unistd.h>
int main(void) {
    const char m[] = "hello_dyn: ALL PASS\n";
    write(1, m, sizeof m - 1);
    return 0;
}
