//! seccomp-bpf syscall filtering for Linux sandboxing
//!
//! This module provides syscall filtering using seccomp-bpf to restrict
//! which system calls sandboxed processes can make.

use seccompiler::{
    BpfProgram, SeccompAction, SeccompCondition, SeccompFilter, SeccompRule, SeccompCmpArgLen,
    SeccompCmpOp,
};

/// Apply a seccomp filter that allows basic operations but blocks dangerous syscalls
///
/// This uses an allow-list approach, permitting only essential syscalls
/// and denying everything else with EPERM.
pub fn apply_seccomp_filter() -> Result<(), Box<dyn std::error::Error>> {
    // Define allowed syscalls with their conditions
    let mut rules: Vec<(i64, Vec<SeccompRule>)> = vec![
        // File operations
        (libc::SYS_read, vec![]),
        (libc::SYS_write, vec![]),
        (libc::SYS_openat, vec![]),
        (libc::SYS_close, vec![]),
        (libc::SYS_newfstatat, vec![]),
        (libc::SYS_fstat, vec![]),
        (libc::SYS_lseek, vec![]),
        (libc::SYS_pread64, vec![]),
        (libc::SYS_pwrite64, vec![]),
        (libc::SYS_readv, vec![]),
        (libc::SYS_writev, vec![]),
        (libc::SYS_preadv, vec![]),
        (libc::SYS_pwritev, vec![]),
        (libc::SYS_dup, vec![]),
        (libc::SYS_dup2, vec![]),
        (libc::SYS_dup3, vec![]),

        // Directory operations
        (libc::SYS_getdents64, vec![]),
        (libc::SYS_mkdirat, vec![]),
        (libc::SYS_unlinkat, vec![]),
        (libc::SYS_renameat, vec![]),
        (libc::SYS_renameat2, vec![]),
        (libc::SYS_symlinkat, vec![]),
        (libc::SYS_readlinkat, vec![]),
        (libc::SYS_faccessat, vec![]),
        (libc::SYS_faccessat2, vec![]),
        (libc::SYS_chdir, vec![]),
        (libc::SYS_fchdir, vec![]),
        (libc::SYS_getcwd, vec![]),

        // Memory operations
        (libc::SYS_mmap, vec![]),
        (libc::SYS_munmap, vec![]),
        (libc::SYS_mprotect, vec![]),
        (libc::SYS_brk, vec![]),
        (libc::SYS_mremap, vec![]),

        // Process control
        (libc::SYS_exit, vec![]),
        (libc::SYS_exit_group, vec![]),
        (libc::SYS_rt_sigaction, vec![]),
        (libc::SYS_rt_sigprocmask, vec![]),
        (libc::SYS_rt_sigreturn, vec![]),
        (libc::SYS_rt_sigtimedwait, vec![]),
        (libc::SYS_kill, vec![]),
        (libc::SYS_tkill, vec![]),
        (libc::SYS_tgkill, vec![]),
        (libc::SYS_getpid, vec![]),
        (libc::SYS_getppid, vec![]),
        (libc::SYS_gettid, vec![]),
        (libc::SYS_getuid, vec![]),
        (libc::SYS_getgid, vec![]),
        (libc::SYS_geteuid, vec![]),
        (libc::SYS_getegid, vec![]),
        (libc::SYS_getgroups, vec![]),
        (libc::SYS_setuid, vec![]),
        (libc::SYS_setgid, vec![]),
        (libc::SYS_setresuid, vec![]),
        (libc::SYS_setresgid, vec![]),
        (libc::SYS_wait4, vec![]),
        (libc::SYS_clone, vec![]),
        (libc::SYS_clone3, vec![]),
        (libc::SYS_fork, vec![]),
        (libc::SYS_vfork, vec![]),
        (libc::SYS_execve, vec![]),
        (libc::SYS_execveat, vec![]),
        (libc::SYS_waitid, vec![]),

        // Signal handling
        (libc::SYS_sigaltstack, vec![]),
        (libc::SYS_signalfd, vec![]),
        (libc::SYS_signalfd4, vec![]),
        (libc::SYS_restart_syscall, vec![]),

        // Time
        (libc::SYS_clock_gettime, vec![]),
        (libc::SYS_clock_getres, vec![]),
        (libc::SYS_clock_nanosleep, vec![]),
        (libc::SYS_gettimeofday, vec![]),
        (libc::SYS_nanosleep, vec![]),
        (libc::SYS_times, vec![]),

        // epoll/poll for async I/O
        (libc::SYS_epoll_create1, vec![]),
        (libc::SYS_epoll_ctl, vec![]),
        (libc::SYS_epoll_pwait, vec![]),
        (libc::SYS_epoll_pwait2, vec![]),
        (libc::SYS_poll, vec![]),
        (libc::SYS_ppoll, vec![]),
        (libc::SYS_select, vec![]),
        (libc::SYS_pselect6, vec![]),

        // Pipes and FIFOs
        (libc::SYS_pipe, vec![]),
        (libc::SYS_pipe2, vec![]),
        (libc::SYS_tee, vec![]),
        (libc::SYS_splice, vec![]),
        (libc::SYS_vmsplice, vec![]),

        // File control
        (libc::SYS_fcntl, vec![]),
        (libc::SYS_ioctl, vec![]),  // Needed for terminals
        (libc::SYS_fsync, vec![]),
        (libc::SYS_fdatasync, vec![]),
        (libc::SYS_sync_file_range, vec![]),
        (libc::SYS_ftruncate, vec![]),
        (libc::SYS_fallocate, vec![]),
        (libc::SYS_fadvise64, vec![]),

        // Eventfd
        (libc::SYS_eventfd, vec![]),
        (libc::SYS_eventfd2, vec![]),

        // Timerfd
        (libc::SYS_timerfd_create, vec![]),
        (libc::SYS_timerfd_settime, vec![]),
        (libc::SYS_timerfd_gettime, vec![]),

        // Inotify (for file watching)
        (libc::SYS_inotify_init, vec![]),
        (libc::SYS_inotify_init1, vec![]),
        (libc::SYS_inotify_add_watch, vec![]),
        (libc::SYS_inotify_rm_watch, vec![]),

        // Fanotify
        (libc::SYS_fanotify_init, vec![]),
        (libc::SYS_fanotify_mark, vec![]),

        // Capabilities
        (libc::SYS_capget, vec![]),
        (libc::SYS_capset, vec![]),
        (libc::SYS_prctl, vec![]),

        // Uname and system info
        (libc::SYS_uname, vec![]),
        (libc::SYS_sysinfo, vec![]),

        // Random
        (libc::SYS_getrandom, vec![]),

        // Umask
        (libc::SYS_umask, vec![]),
        (libc::SYS_chmod, vec![]),
        (libc::SYS_fchmod, vec![]),
        (libc::SYS_fchmodat, vec![]),

        // Extended attributes
        (libc::SYS_setxattr, vec![]),
        (libc::SYS_lsetxattr, vec![]),
        (libc::SYS_fsetxattr, vec![]),
        (libc::SYS_getxattr, vec![]),
        (libc::SYS_lgetxattr, vec![]),
        (libc::SYS_fgetxattr, vec![]),
        (libc::SYS_listxattr, vec![]),
        (libc::SYS_llistxattr, vec![]),
        (libc::SYS_flistxattr, vec![]),
        (libc::SYS_removexattr, vec![]),
        (libc::SYS_lremovexattr, vec![]),
        (libc::SYS_fremovexattr, vec![]),

        // Rlimit
        (libc::SYS_getrlimit, vec![]),
        (libc::SYS_setrlimit, vec![]),
        (libc::SYS_prlimit64, vec![]),

        // Resource usage
        (libc::SYS_getrusage, vec![]),

        // Uid/Gid mapping for user namespaces
        (libc::SYS_setuid, vec![]),
        (libc::SYS_setgid, vec![]),
        (libc::SYS_setgroups, vec![]),

        // Ptrace (may be restricted further in future)
        (libc::SYS_ptrace, vec![]),

        // Seccomp
        (libc::SYS_seccomp, vec![]),

        // Landlock
        (libc::SYS_landlock_create_ruleset, vec![]),
        (libc::SYS_landlock_add_rule, vec![]),
        (libc::SYS_landlock_restrict_self, vec![]),

        // Socket operations (for network-enabled mode)
        (libc::SYS_socket, vec![]),
        (libc::SYS_socketpair, vec![]),
        (libc::SYS_bind, vec![]),
        (libc::SYS_listen, vec![]),
        (libc::SYS_accept, vec![]),
        (libc::SYS_accept4, vec![]),
        (libc::SYS_connect, vec![]),
        (libc::SYS_shutdown, vec![]),
        (libc::SYS_getsockname, vec![]),
        (libc::SYS_getpeername, vec![]),
        (libc::SYS_getsockopt, vec![]),
        (libc::SYS_setsockopt, vec![]),
        (libc::SYS_sendto, vec![]),
        (libc::SYS_recvfrom, vec![]),
        (libc::SYS_sendmsg, vec![]),
        (libc::SYS_recvmsg, vec![]),
        (libc::SYS_sendmmsg, vec![]),
        (libc::SYS_recvmmsg, vec![]),

        // IO_uring (if available)
        (libc::SYS_io_uring_setup, vec![]),
        (libc::SYS_io_uring_enter, vec![]),
        (libc::SYS_io_uring_register, vec![]),
    ];

    // Create the filter with EPERM as the default action for denied syscalls
    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Errno(libc::EPERM), // Deny with EPERM
        SeccompAction::Allow,
        std::env::consts::ARCH.try_into()?,
    )?;

    let program = filter.compile()?;

    // Load the seccomp filter
    unsafe {
        // Enable no_new_privs (required for seccomp)
        let result = libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
        if result < 0 {
            return Err(format!(
                "prctl(PR_SET_NO_NEW_PRIVS) failed: {}",
                std::io::Error::last_os_error()
            )
            .into());
        }

        // Apply the seccomp filter
        let result = libc::prctl(
            libc::PR_SET_SECCOMP,
            libc::SECCOMP_MODE_FILTER,
            program.as_ptr() as *const _,
        );
        if result < 0 {
            return Err(format!(
                "prctl(PR_SET_SECCOMP) failed: {}",
                std::io::Error::last_os_error()
            )
            .into());
        }
    }

    Ok(())
}

/// Apply a restrictive seccomp filter for network-disabled mode
///
/// This filter removes socket-related syscalls while keeping other
/// essential operations.
pub fn apply_restrictive_seccomp() -> Result<(), Box<dyn std::error::Error>> {
    // Define allowed syscalls without socket operations
    let rules: Vec<(i64, Vec<SeccompRule>)> = vec![
        // File operations
        (libc::SYS_read, vec![]),
        (libc::SYS_write, vec![]),
        (libc::SYS_openat, vec![]),
        (libc::SYS_close, vec![]),
        (libc::SYS_newfstatat, vec![]),
        (libc::SYS_fstat, vec![]),
        (libc::SYS_lseek, vec![]),
        (libc::SYS_pread64, vec![]),
        (libc::SYS_pwrite64, vec![]),
        (libc::SYS_readv, vec![]),
        (libc::SYS_writev, vec![]),
        (libc::SYS_dup, vec![]),
        (libc::SYS_dup2, vec![]),
        (libc::SYS_dup3, vec![]),

        // Directory operations
        (libc::SYS_getdents64, vec![]),
        (libc::SYS_mkdirat, vec![]),
        (libc::SYS_unlinkat, vec![]),
        (libc::SYS_renameat, vec![]),
        (libc::SYS_renameat2, vec![]),
        (libc::SYS_faccessat, vec![]),
        (libc::SYS_faccessat2, vec![]),
        (libc::SYS_chdir, vec![]),
        (libc::SYS_fchdir, vec![]),
        (libc::SYS_getcwd, vec![]),

        // Memory operations
        (libc::SYS_mmap, vec![]),
        (libc::SYS_munmap, vec![]),
        (libc::SYS_mprotect, vec![]),
        (libc::SYS_brk, vec![]),
        (libc::SYS_mremap, vec![]),

        // Process control
        (libc::SYS_exit, vec![]),
        (libc::SYS_exit_group, vec![]),
        (libc::SYS_rt_sigaction, vec![]),
        (libc::SYS_rt_sigprocmask, vec![]),
        (libc::SYS_rt_sigreturn, vec![]),
        (libc::SYS_rt_sigtimedwait, vec![]),
        (libc::SYS_getpid, vec![]),
        (libc::SYS_getppid, vec![]),
        (libc::SYS_gettid, vec![]),
        (libc::SYS_getuid, vec![]),
        (libc::SYS_getgid, vec![]),
        (libc::SYS_geteuid, vec![]),
        (libc::SYS_getegid, vec![]),
        (libc::SYS_wait4, vec![]),
        (libc::SYS_clone, vec![]),
        (libc::SYS_clone3, vec![]),
        (libc::SYS_fork, vec![]),
        (libc::SYS_vfork, vec![]),
        (libc::SYS_execve, vec![]),
        (libc::SYS_execveat, vec![]),
        (libc::SYS_waitid, vec![]),

        // Signal handling
        (libc::SYS_sigaltstack, vec![]),
        (libc::SYS_restart_syscall, vec![]),

        // Time
        (libc::SYS_clock_gettime, vec![]),
        (libc::SYS_clock_getres, vec![]),
        (libc::SYS_clock_nanosleep, vec![]),
        (libc::SYS_gettimeofday, vec![]),
        (libc::SYS_nanosleep, vec![]),
        (libc::SYS_times, vec![]),

        // epoll/poll for async I/O
        (libc::SYS_epoll_create1, vec![]),
        (libc::SYS_epoll_ctl, vec![]),
        (libc::SYS_epoll_pwait, vec![]),
        (libc::SYS_epoll_pwait2, vec![]),
        (libc::SYS_poll, vec![]),
        (libc::SYS_ppoll, vec![]),
        (libc::SYS_select, vec![]),
        (libc::SYS_pselect6, vec![]),

        // Pipes and FIFOs
        (libc::SYS_pipe, vec![]),
        (libc::SYS_pipe2, vec![]),
        (libc::SYS_tee, vec![]),
        (libc::SYS_splice, vec![]),
        (libc::SYS_vmsplice, vec![]),

        // File control
        (libc::SYS_fcntl, vec![]),
        (libc::SYS_ioctl, vec![]),
        (libc::SYS_fsync, vec![]),
        (libc::SYS_fdatasync, vec![]),
        (libc::SYS_ftruncate, vec![]),

        // Eventfd
        (libc::SYS_eventfd, vec![]),
        (libc::SYS_eventfd2, vec![]),

        // Timerfd
        (libc::SYS_timerfd_create, vec![]),
        (libc::SYS_timerfd_settime, vec![]),
        (libc::SYS_timerfd_gettime, vec![]),

        // Capabilities
        (libc::SYS_capget, vec![]),
        (libc::SYS_capset, vec![]),
        (libc::SYS_prctl, vec![]),

        // Uname and system info
        (libc::SYS_uname, vec![]),
        (libc::SYS_sysinfo, vec![]),

        // Random
        (libc::SYS_getrandom, vec![]),

        // Umask
        (libc::SYS_umask, vec![]),
        (libc::SYS_chmod, vec![]),
        (libc::SYS_fchmod, vec![]),
        (libc::SYS_fchmodat, vec![]),

        // Rlimit
        (libc::SYS_getrlimit, vec![]),
        (libc::SYS_setrlimit, vec![]),
        (libc::SYS_prlimit64, vec![]),

        // Resource usage
        (libc::SYS_getrusage, vec![]),

        // Seccomp
        (libc::SYS_seccomp, vec![]),

        // Landlock
        (libc::SYS_landlock_create_ruleset, vec![]),
        (libc::SYS_landlock_add_rule, vec![]),
        (libc::SYS_landlock_restrict_self, vec![]),
    ];

    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Errno(libc::EPERM),
        SeccompAction::Allow,
        std::env::consts::ARCH.try_into()?,
    )?;

    let program = filter.compile()?;

    unsafe {
        let result = libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
        if result < 0 {
            return Err(format!(
                "prctl(PR_SET_NO_NEW_PRIVS) failed: {}",
                std::io::Error::last_os_error()
            )
            .into());
        }

        let result = libc::prctl(
            libc::PR_SET_SECCOMP,
            libc::SECCOMP_MODE_FILTER,
            program.as_ptr() as *const _,
        );
        if result < 0 {
            return Err(format!(
                "prctl(PR_SET_SECCOMP) failed: {}",
                std::io::Error::last_os_error()
            )
            .into());
        }
    }

    Ok(())
}

/// Check if seccomp is available on this system
pub fn is_seccomp_available() -> bool {
    unsafe {
        // Try to check if seccomp is supported by attempting to set no_new_privs
        // and checking for SECCOMP_MODE_FILTER support
        let result = libc::prctl(libc::PR_GET_SECCOMP, 0, 0, 0, 0);
        result >= 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ENOSYS)
    }
}

/// Get the current seccomp mode
pub fn get_seccomp_mode() -> i32 {
    unsafe { libc::prctl(libc::PR_GET_SECCOMP, 0, 0, 0, 0) }
}
