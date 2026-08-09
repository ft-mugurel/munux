/* P10d: glibc clone() trampoline (fn+arg on child stack). SETTLS is tlsclone. */
#define _GNU_SOURCE
#include <sched.h>
#include <signal.h>
#include <sys/wait.h>
#include <unistd.h>

static int worker(void *arg) {
    (void)arg;
    const char m[] = "clonec: child ran\n";
    write(1, m, sizeof m - 1);
    return 0;
}

int main(void) {
    static char stack[8192];
    int tid;
    int st = 0;
    tid = clone(worker, stack + sizeof stack,
                CLONE_VM | CLONE_FS | CLONE_FILES | CLONE_SIGHAND | SIGCHLD, 0);
    if (tid < 0) {
        const char e[] = "clonec FAIL clone\n";
        write(2, e, sizeof e - 1);
        return 1;
    }
    if (waitpid(tid, &st, 0) < 0 || !WIFEXITED(st) || WEXITSTATUS(st) != 0) {
        const char e[] = "clonec FAIL wait\n";
        write(2, e, sizeof e - 1);
        return 1;
    }
    const char o[] = "clonec: ALL PASS\n";
    write(1, o, sizeof o - 1);
    return 0;
}
