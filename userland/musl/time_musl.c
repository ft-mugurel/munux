/* Static musl time smoke: clock_gettime + time. */
#include <stdio.h>
#include <time.h>

int main(void) {
    struct timespec ts;
    if (clock_gettime(CLOCK_REALTIME, &ts) != 0) {
        printf("clock_gettime failed\n");
        return 1;
    }
    time_t t = time(NULL);
    printf("realtime sec=%lld nsec=%ld\n", (long long)ts.tv_sec, (long)ts.tv_nsec);
    printf("time()=%lld\n", (long long)t);
    printf("time_musl: OK\n");
    return 0;
}
