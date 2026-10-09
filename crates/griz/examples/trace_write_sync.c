// Observe real Darwin sync and rename calls. Forward known fcntl signatures;
// reject unknown signatures so the proof fails instead of forwarding wrong types.
// GRIZ_RENAME_FAIL_AFTER optionally injects EIO after that many staged renames.
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdlib.h>
#include <stdio.h>
#include <errno.h>
#include <pthread.h>
#include <string.h>

static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;

static void record(const char *kind, int fd, int result, const char *path) {
    const char *trace = getenv("GRIZ_SYNC_TRACE");
    if (!trace) return;
    char resolved[1024] = "";
    struct stat metadata = {0};
    if (fd >= 0) {
        fcntl(fd, F_GETPATH, resolved);
        fstat(fd, &metadata);
        path = resolved;
    }
    char line[2300];
    int length = snprintf(line, sizeof(line),
        "{\"kind\":\"%s\",\"device\":%llu,\"result\":%d,\"path\":\"%s\"}\n",
        kind, (unsigned long long)metadata.st_dev, result, path ? path : "");
    pthread_mutex_lock(&lock);
    int output = open(trace, O_WRONLY | O_CREAT | O_APPEND, 0600);
    if (output >= 0) {
        if (length > 0 && length < (int)sizeof(line)) write(output, line, length);
        close(output);
    }
    pthread_mutex_unlock(&lock);
}

static int observed_fsync(int fd) {
    int result = fsync(fd);
    int saved = errno;
    record("fsync", fd, result, NULL);
    errno = saved;
    return result;
}

static int observed_fcntl(int fd, int command, ...) {
    int result;
    va_list args;
    va_start(args, command);
    switch (command) {
    case F_FULLFSYNC:
    case F_BARRIERFSYNC:
    case F_GETFD:
    case F_GETFL:
        result = fcntl(fd, command);
        break;
    case F_SETFD:
    case F_SETFL:
    case F_DUPFD:
    case F_DUPFD_CLOEXEC:
        result = fcntl(fd, command, va_arg(args, int));
        break;
    case F_GETPATH:
    case F_GETLK:
    case F_SETLK:
    case F_SETLKW:
        result = fcntl(fd, command, va_arg(args, void *));
        break;
    default:
        record("unsupported_fcntl", fd, command, NULL);
        errno = ENOTSUP;
        result = -1;
    }
    va_end(args);
    int saved = errno;
    if (command == F_FULLFSYNC) record("full_sync", fd, result, NULL);
    errno = saved;
    return result;
}

static int reject_rename(const char *path) {
    static unsigned long calls = 0;
    const char *limit = getenv("GRIZ_RENAME_FAIL_AFTER");
    if (!limit || !strstr(path, ".griz-")) return 0;
    pthread_mutex_lock(&lock);
    int reject = calls++ >= strtoul(limit, NULL, 10);
    pthread_mutex_unlock(&lock);
    return reject;
}

static int observed_rename(const char *old_path, const char *new_path) {
    int injected = reject_rename(old_path);
    int result;
    if (injected) {
        errno = EIO;
        result = -1;
    } else {
        result = rename(old_path, new_path);
    }
    int saved = errno;
    record(injected ? "rename_injected_error" : "rename", -1, result, old_path);
    errno = saved;
    return result;
}

#define INTERPOSE(replacement, original) \
    __attribute__((used)) static struct { const void *replacement; const void *original; } \
    interpose_##original __attribute__((section("__DATA,__interpose"))) = { \
        (const void *)&replacement, (const void *)&original \
    }
INTERPOSE(observed_fsync, fsync);
INTERPOSE(observed_fcntl, fcntl);
INTERPOSE(observed_rename, rename);
