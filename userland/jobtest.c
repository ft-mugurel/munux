/* P11a: session/pgrp + console termios (isatty / tcgetattr / setsid). */
#include <sys/ioctl.h>
#include <sys/wait.h>
#include <termios.h>
#include <unistd.h>

static void fail(const char *s, unsigned n) {
    write(2, s, n);
    _exit(1);
}

int main(void) {
    struct termios t;
    struct winsize ws;
    pid_t me, c;
    int st = 0;
    int fg = 0;

    if (!isatty(0) || !isatty(1)) {
        fail("jobtest FAIL isatty\n", 20);
    }
    if (tcgetattr(0, &t) != 0) {
        fail("jobtest FAIL tcgetattr\n", 23);
    }
    if ((t.c_lflag & ICANON) == 0 || (t.c_lflag & ECHO) == 0) {
        fail("jobtest FAIL termios\n", 21);
    }
    if (ioctl(0, TIOCGWINSZ, &ws) != 0 || ws.ws_col != 80 || ws.ws_row != 25) {
        fail("jobtest FAIL winsize\n", 21);
    }

    me = getpid();
    if (getpgrp() <= 0 || getsid(0) <= 0 || getpgid(0) != getpgrp()) {
        fail("jobtest FAIL pgrp\n", 18);
    }
    if (setpgid(0, 0) != 0 || getpgrp() != me) {
        fail("jobtest FAIL setpgid\n", 21);
    }

    c = fork();
    if (c < 0) {
        fail("jobtest FAIL fork\n", 18);
    }
    if (c == 0) {
        if (setsid() < 0) {
            fail("jobtest FAIL setsid\n", 20);
        }
        if (getpid() != getpgrp() || getpgrp() != getsid(0)) {
            fail("jobtest FAIL child ids\n", 23);
        }
        if (ioctl(0, TIOCSCTTY, 0) != 0) {
            fail("jobtest FAIL TIOCSCTTY\n", 23);
        }
        if (ioctl(0, TIOCGPGRP, &fg) != 0 || fg != getpgrp()) {
            fail("jobtest FAIL TIOCGPGRP\n", 23);
        }
        {
            const char m[] = "jobtest: child session OK\n";
            write(1, m, sizeof m - 1);
        }
        _exit(0);
    }
    if (waitpid(c, &st, 0) < 0 || !WIFEXITED(st) || WEXITSTATUS(st) != 0) {
        fail("jobtest FAIL wait\n", 18);
    }
    {
        const char o[] = "jobtest: ALL PASS\n";
        write(1, o, sizeof o - 1);
    }
    return 0;
}
