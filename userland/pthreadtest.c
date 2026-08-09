/* P10d: glibc pthread_create + join (libc.so.6, no separate libpthread). */
#include <pthread.h>
#include <unistd.h>

static void *worker(void *arg) {
    (void)arg;
    const char m[] = "pth B: thread ran\n";
    write(1, m, sizeof m - 1);
    return 0;
}

int main(void) {
    pthread_t th;
    pthread_attr_t attr;
    const char a[] = "pth A: create\n";
    write(1, a, sizeof a - 1);
    pthread_attr_init(&attr);
    pthread_attr_setstacksize(&attr, 64 * 1024);
    if (pthread_create(&th, &attr, worker, 0) != 0) {
        const char e[] = "pthreadtest FAIL create\n";
        write(2, e, sizeof e - 1);
        return 1;
    }
    pthread_attr_destroy(&attr);
    if (pthread_join(th, 0) != 0) {
        const char e[] = "pthreadtest FAIL join\n";
        write(2, e, sizeof e - 1);
        return 1;
    }
    const char o[] = "pthreadtest: ALL PASS\n";
    write(1, o, sizeof o - 1);
    return 0;
}
