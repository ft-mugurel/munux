/* Static musl smoke — host build: see Makefile musl-hello target / docs. */
#include <unistd.h>
int main(void) {
    const char msg[] = "hello from musl\n";
    write(1, msg, sizeof msg - 1);
    return 0;
}
