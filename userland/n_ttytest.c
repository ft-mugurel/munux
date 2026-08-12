/* P11c: n_tty on a PTY — canonical erase + ISIG Ctrl-C. */
#include <fcntl.h>
#include <signal.h>
#include <sys/ioctl.h>
#include <sys/wait.h>
#include <unistd.h>

#ifndef TIOCGPTN
#define TIOCGPTN 0x80045430
#endif
#ifndef TIOCSPTLCK
#define TIOCSPTLCK 0x40045431
#endif
#ifndef TIOCSCTTY
#define TIOCSCTTY 0x540E
#endif

static void fail(const char *s, unsigned n) {
    write(2, s, n);
    _exit(1);
}

static int open_ptmx(int *n_out, char *path) {
    int m = open("/dev/ptmx", O_RDWR);
    int n = -1;
    int unlock = 0;
    if (m < 0) {
        fail("n_ttytest FAIL open ptmx\n", 24);
    }
    if (ioctl(m, TIOCGPTN, &n) != 0 || n < 0 || n > 9) {
        fail("n_ttytest FAIL TIOCGPTN\n", 23);
    }
    if (ioctl(m, TIOCSPTLCK, &unlock) != 0) {
        fail("n_ttytest FAIL unlock\n", 22);
    }
    path[0] = '/';
    path[1] = 'd';
    path[2] = 'e';
    path[3] = 'v';
    path[4] = '/';
    path[5] = 'p';
    path[6] = 't';
    path[7] = 's';
    path[8] = '/';
    path[9] = (char)('0' + n);
    path[10] = 0;
    *n_out = n;
    return m;
}

int main(void) {
    int m, dummy, s, st, nr;
    pid_t c;
    char path[16];
    char buf[32];

    /* ---- A: "ab" + DEL + "c" + NL → slave reads "ac\n" ---- */
    m = open_ptmx(&dummy, path);
    if (write(m, "ab\x7f" "c\n", 5) != 5) {
        fail("n_ttytest FAIL write A\n", 23);
    }
    c = fork();
    if (c < 0) {
        fail("n_ttytest FAIL fork A\n", 22);
    }
    if (c == 0) {
        close(m);
        if (setsid() < 0) {
            fail("n_ttytest FAIL setsid A\n", 24);
        }
        s = open(path, O_RDWR);
        if (s < 0) {
            fail("n_ttytest FAIL open pts A\n", 26);
        }
        ioctl(s, TIOCSCTTY, 0);
        if (dup2(s, 0) < 0 || dup2(s, 1) < 0) {
            fail("n_ttytest FAIL dup2 A\n", 22);
        }
        if (s > 2) {
            close(s);
        }
        nr = read(0, buf, sizeof buf);
        if (nr != 3 || buf[0] != 'a' || buf[1] != 'c' || buf[2] != '\n') {
            fail("n_ttytest FAIL cook\n", 20);
        }
        {
            const char msg[] = "NTY-LINE-OK\n";
            write(1, msg, sizeof msg - 1);
        }
        _exit(0);
    }
    if (waitpid(c, &st, 0) < 0 || !WIFEXITED(st) || WEXITSTATUS(st) != 0) {
        fail("n_ttytest FAIL wait A\n", 22);
    }
    close(m);

    /* ---- B: after TIOCSCTTY, write ^C on master → SIGINT ---- */
    m = open_ptmx(&dummy, path);
    c = fork();
    if (c < 0) {
        fail("n_ttytest FAIL fork B\n", 22);
    }
    if (c == 0) {
        if (setsid() < 0) {
            fail("n_ttytest FAIL setsid B\n", 24);
        }
        s = open(path, O_RDWR);
        if (s < 0) {
            fail("n_ttytest FAIL open pts B\n", 26);
        }
        if (ioctl(s, TIOCSCTTY, 0) != 0) {
            fail("n_ttytest FAIL TIOCSCTTY\n", 25);
        }
        /* Keep master; cook ^C against our new fg pgrp. */
        if (write(m, "\x03", 1) != 1) {
            fail("n_ttytest FAIL write C-c\n", 25);
        }
        fail("n_ttytest FAIL still alive\n", 27);
    }
    if (waitpid(c, &st, 0) < 0) {
        fail("n_ttytest FAIL wait B\n", 22);
    }
    if (WIFEXITED(st) && WEXITSTATUS(st) == 128 + SIGINT) {
        /* munux wait encodes fatal as exit(128+sig) */
    } else if (WIFSIGNALED(st) && WTERMSIG(st) == SIGINT) {
        /* Linux-style signal death */
    } else {
        fail("n_ttytest FAIL SIGINT\n", 22);
    }
    close(m);

    {
        const char o[] = "n_ttytest: ALL PASS\n";
        write(1, o, sizeof o - 1);
    }
    return 0;
}
