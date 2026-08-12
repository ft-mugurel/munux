/* P11b: Unix98 PTY — /dev/ptmx + /dev/pts/N byte bridge.
 * P11c: default n_tty is canonical — this smoke turns ICANON off for a 1-byte read. */
#include <fcntl.h>
#include <sys/ioctl.h>
#include <sys/wait.h>
#include <termios.h>
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

int main(void) {
    int m, n, unlock, s, st;
    pid_t c;
    char path[16];
    char buf[32];
    int nr;

    m = open("/dev/ptmx", O_RDWR);
    if (m < 0) {
        fail("ptytest FAIL open ptmx\n", 22);
    }
    n = -1;
    if (ioctl(m, TIOCGPTN, &n) != 0 || n < 0 || n > 9) {
        fail("ptytest FAIL TIOCGPTN\n", 21);
    }
    unlock = 0;
    if (ioctl(m, TIOCSPTLCK, &unlock) != 0) {
        fail("ptytest FAIL unlock\n", 20);
    }

    /* /dev/pts/N */
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

    /* Data ready before the child runs (wait4 schedules it). */
    if (write(m, "x", 1) != 1) {
        fail("ptytest FAIL write\n", 19);
    }

    c = fork();
    if (c < 0) {
        fail("ptytest FAIL fork\n", 18);
    }
    if (c == 0) {
        close(m);
        if (setsid() < 0) {
            fail("ptytest FAIL setsid\n", 20);
        }
        s = open(path, O_RDWR);
        if (s < 0) {
            fail("ptytest FAIL open pts\n", 22);
        }
        if (!isatty(s)) {
            fail("ptytest FAIL isatty\n", 20);
        }
        {
            struct termios t;
            if (tcgetattr(s, &t) != 0) {
                fail("ptytest FAIL tcgetattr\n", 23);
            }
            t.c_lflag &= ~(tcflag_t)(ICANON | ECHO);
            t.c_oflag &= ~(tcflag_t)OPOST;
            t.c_cc[VMIN] = 1;
            t.c_cc[VTIME] = 0;
            if (tcsetattr(s, TCSANOW, &t) != 0) {
                fail("ptytest FAIL tcsetattr\n", 23);
            }
        }
        if (ioctl(s, TIOCSCTTY, 0) != 0) {
            fail("ptytest FAIL TIOCSCTTY\n", 23);
        }
        if (dup2(s, 0) < 0 || dup2(s, 1) < 0 || dup2(s, 2) < 0) {
            fail("ptytest FAIL dup2\n", 18);
        }
        if (s > 2) {
            close(s);
        }
        {
            char b[4];
            if (read(0, b, 1) != 1 || b[0] != 'x') {
                fail("ptytest FAIL child read\n", 24);
            }
        }
        {
            const char msg[] = "PTY-CHILD-OK\n";
            write(1, msg, sizeof msg - 1);
        }
        _exit(0);
    }

    if (waitpid(c, &st, 0) < 0 || !WIFEXITED(st) || WEXITSTATUS(st) != 0) {
        fail("ptytest FAIL wait\n", 18);
    }
    nr = read(m, buf, sizeof buf);
    if (nr < 13) {
        fail("ptytest FAIL read\n", 18);
    }
    {
        const char want[] = "PTY-CHILD-OK\n";
        int i;
        int ok = 0;
        for (i = 0; i + 13 <= nr; i++) {
            int j;
            int mtc = 1;
            for (j = 0; j < 13; j++) {
                if (buf[i + j] != want[j]) {
                    mtc = 0;
                    break;
                }
            }
            if (mtc) {
                ok = 1;
                break;
            }
        }
        if (!ok) {
            fail("ptytest FAIL data\n", 18);
        }
    }
    {
        const char o[] = "ptytest: ALL PASS\n";
        write(1, o, sizeof o - 1);
    }
    return 0;
}
