/* P11d: background tty read/write stops (SIGTTIN / SIGTTOU) + SIGCONT. */
#include <fcntl.h>
#include <signal.h>
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

#ifndef WUNTRACED
#define WUNTRACED 2
#endif

static void fail(const char *s, unsigned n) {
    write(2, s, n);
    _exit(1);
}

int main(void) {
    int m, n, unlock, s, st;
    pid_t c;
    char path[16];

    m = open("/dev/ptmx", O_RDWR);
    if (m < 0) {
        fail("jobstop FAIL ptmx\n", 17);
    }
    n = -1;
    if (ioctl(m, TIOCGPTN, &n) != 0 || n < 0 || n > 9) {
        fail("jobstop FAIL TIOCGPTN\n", 22);
    }
    unlock = 0;
    ioctl(m, TIOCSPTLCK, &unlock);
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

    c = fork();
    if (c < 0) {
        fail("jobstop FAIL fork\n", 18);
    }
    if (c == 0) {
        pid_t gc, gc2;
        close(m);
        if (setsid() < 0) {
            fail("jobstop FAIL setsid\n", 20);
        }
        s = open(path, O_RDWR);
        if (s < 0) {
            fail("jobstop FAIL pts\n", 17);
        }
        if (ioctl(s, TIOCSCTTY, 0) != 0) {
            fail("jobstop FAIL TIOCSCTTY\n", 23);
        }
        dup2(s, 0);
        dup2(s, 1);
        dup2(s, 2);
        if (s > 2) {
            close(s);
        }

        gc = fork();
        if (gc < 0) {
            _exit(10);
        }
        if (gc == 0) {
            if (setpgid(0, 0) != 0) {
                _exit(11);
            }
            {
                char b;
                read(0, &b, 1); /* SIGTTIN — should not return */
            }
            _exit(12);
        }
        if (waitpid(gc, &st, WUNTRACED) < 0 || !WIFSTOPPED(st) || WSTOPSIG(st) != SIGTTIN) {
            _exit(13);
        }
        kill(gc, SIGCONT);
        kill(gc, SIGKILL);
        waitpid(gc, &st, 0);

        {
            struct termios t;
            if (tcgetattr(1, &t) != 0) {
                _exit(14);
            }
            t.c_lflag |= (tcflag_t)TOSTOP;
            if (tcsetattr(1, TCSANOW, &t) != 0) {
                _exit(15);
            }
        }
        gc2 = fork();
        if (gc2 < 0) {
            _exit(16);
        }
        if (gc2 == 0) {
            if (setpgid(0, 0) != 0) {
                _exit(17);
            }
            write(1, "x", 1); /* SIGTTOU (TOSTOP) */
            _exit(18);
        }
        if (waitpid(gc2, &st, WUNTRACED) < 0 || !WIFSTOPPED(st) || WSTOPSIG(st) != SIGTTOU) {
            _exit(19);
        }
        kill(gc2, SIGKILL);
        waitpid(gc2, &st, 0);
        _exit(0);
    }

    if (waitpid(c, &st, 0) < 0 || !WIFEXITED(st) || WEXITSTATUS(st) != 0) {
        fail("jobstop FAIL wait\n", 18);
    }
    {
        const char o[] = "jobstoptest: ALL PASS\n";
        write(1, o, sizeof o - 1);
    }
    return 0;
}
