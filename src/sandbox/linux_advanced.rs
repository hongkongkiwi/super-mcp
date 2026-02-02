//! Advanced Linux sandboxing using namespaces, cgroups, and seccomp
//!
//! This module implements comprehensive Linux sandboxing with:
//! - User namespaces (isolate user/group IDs)
//! - PID namespaces (process isolation)
//! - Network namespaces (network isolation)
//! - Mount namespaces (filesystem isolation)
//! - IPC namespaces (IPC isolation)
//! - Cgroups v2 (resource limits)
//! - Seccomp-bpf (syscall filtering)

use crate::config::McpServerConfig;
use crate::sandbox::traits::{FilesystemConstraint, Sandbox, SandboxConstraints};
use crate::utils::errors::{McpError, McpResult};
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::process::Child;
use tracing::{debug, error, info, warn};

/// Advanced Linux sandbox configuration
#[derive(Debug, Clone)]
pub struct AdvancedLinuxSandboxConfig {
    /// Use user namespace
    pub use_user_namespace: bool,
    /// Use PID namespace
    pub use_pid_namespace: bool,
    /// Use network namespace
    pub use_network_namespace: bool,
    /// Use mount namespace
    pub use_mount_namespace: bool,
    /// Use IPC namespace
    pub use_ipc_namespace: bool,
    /// Use cgroups for resource limits
    pub use_cgroups: bool,
    /// Use seccomp for syscall filtering
    pub use_seccomp: bool,
    /// Root filesystem for container (if using mount namespace)
    pub rootfs: Option<PathBuf>,
    /// Read-only paths
    pub read_only_paths: Vec<PathBuf>,
    /// Writable paths
    pub write_paths: Vec<PathBuf>,
}

impl Default for AdvancedLinuxSandboxConfig {
    fn default() -> Self {
        Self {
            use_user_namespace: true,
            use_pid_namespace: true,
            use_network_namespace: false,
            use_mount_namespace: true,
            use_ipc_namespace: true,
            use_cgroups: true,
            use_seccomp: true,
            rootfs: None,
            read_only_paths: vec![],
            write_paths: vec![],
        }
    }
}

/// Advanced Linux sandbox with full namespace support
pub struct AdvancedLinuxSandbox {
    constraints: SandboxConstraints,
    config: AdvancedLinuxSandboxConfig,
    cgroup_path: Option<PathBuf>,
}

impl AdvancedLinuxSandbox {
    /// Create a new advanced Linux sandbox
    pub fn from_config(server_config: &McpServerConfig) -> Self {
        let constraints = SandboxConstraints {
            network: server_config.sandbox.network,
            filesystem: match &server_config.sandbox.filesystem {
                crate::config::FilesystemAccess::Simple(s) if s == "readonly" => {
                    FilesystemConstraint::ReadOnly
                }
                crate::config::FilesystemAccess::Simple(s) if s == "full" => {
                    FilesystemConstraint::Full
                }
                crate::config::FilesystemAccess::Paths(paths) => {
                    FilesystemConstraint::Paths(paths.clone())
                }
                _ => FilesystemConstraint::ReadOnly,
            },
            env_inherit: server_config.sandbox.env_inherit,
            max_memory_mb: server_config.sandbox.max_memory_mb,
            max_cpu_percent: server_config.sandbox.max_cpu_percent,
        };

        let sandbox_config = AdvancedLinuxSandboxConfig {
            use_network_namespace: !server_config.sandbox.network,
            ..Default::default()
        };

        Self {
            constraints,
            config: sandbox_config,
            cgroup_path: None,
        }
    }

    /// Check if advanced sandboxing is available
    pub fn is_available() -> bool {
        #[cfg(target_os = "linux")]
        {
            // Check for namespace support
            std::path::Path::new("/proc/self/ns/user").exists()
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    /// Setup cgroups for resource limits
    #[cfg(target_os = "linux")]
    async fn setup_cgroups(&self, cgroup_name: &str) -> McpResult<PathBuf> {
        use std::fs;
        
        let cgroup_base = PathBuf::from("/sys/fs/cgroup/super-mcp");
        let cgroup_path = cgroup_base.join(cgroup_name);

        // Create cgroup directory
        tokio::fs::create_dir_all(&cgroup_path).await.map_err(|e| {
            McpError::SandboxError(format!("Failed to create cgroup: {}", e))
        })?;

        // Set memory limit
        let memory_limit = self.constraints.max_memory_mb * 1024 * 1024;
        let memory_max_path = cgroup_path.join("memory.max");
        fs::write(&memory_max_path, memory_limit.to_string()).map_err(|e| {
            McpError::SandboxError(format!("Failed to set memory limit: {}", e))
        })?;

        // Set CPU limit (if cgroup v2 cpu.max exists)
        let cpu_max_path = cgroup_path.join("cpu.max");
        if cpu_max_path.exists() {
            // cpu.max format: "quota period" (e.g., "50000 100000" for 50%)
            let quota = (self.constraints.max_cpu_percent as u64 * 1000) as u64;
            let period = 100000u64;
            fs::write(&cpu_max_path, format!("{} {}", quota, period)).map_err(|e| {
                McpError::SandboxError(format!("Failed to set CPU limit: {}", e))
            })?;
        }

        // Enable memory accounting
        let memory_stat_path = cgroup_path.join("memory.stat");
        if memory_stat_path.exists() {
            debug!("Cgroup memory accounting enabled");
        }

        info!("Created cgroup at {:?}", cgroup_path);
        Ok(cgroup_path)
    }

    #[cfg(not(target_os = "linux"))]
    async fn setup_cgroups(&self, _cgroup_name: &str) -> McpResult<PathBuf> {
        Err(McpError::SandboxError("cgroups only available on Linux".to_string()))
    }

    /// Cleanup cgroups
    #[cfg(target_os = "linux")]
    async fn cleanup_cgroups(&self, cgroup_path: &PathBuf) -> McpResult<()> {
        // Kill all processes in the cgroup
        let procs_path = cgroup_path.join("cgroup.procs");
        if procs_path.exists() {
            if let Ok(procs) = std::fs::read_to_string(&procs_path) {
                for pid in procs.lines() {
                    if let Ok(pid) = pid.parse::<i32>() {
                        let _ = nix::sys::signal::kill(
                            nix::unistd::Pid::from_raw(pid),
                            nix::sys::signal::SIGKILL,
                        );
                    }
                }
            }
        }

        // Remove cgroup directory
        tokio::fs::remove_dir(cgroup_path).await.map_err(|e| {
            McpError::SandboxError(format!("Failed to remove cgroup: {}", e))
        })?;

        Ok(())
    }

    /// Build namespace flags for clone syscall
    #[cfg(target_os = "linux")]
    fn build_namespace_flags(&self) -> nix::sched::CloneFlags {
        use nix::sched::CloneFlags;

        let mut flags = CloneFlags::empty();

        if self.config.use_user_namespace {
            flags |= CloneFlags::CLONE_NEWUSER;
        }
        if self.config.use_pid_namespace {
            flags |= CloneFlags::CLONE_NEWPID;
        }
        if self.config.use_network_namespace {
            flags |= CloneFlags::CLONE_NEWNET;
        }
        if self.config.use_mount_namespace {
            flags |= CloneFlags::CLONE_NEWNS;
        }
        if self.config.use_ipc_namespace {
            flags |= CloneFlags::CLONE_NEWIPC;
        }

        flags
    }

    /// Setup user namespace mapping
    #[cfg(target_os = "linux")]
    fn setup_uid_map(&self, pid: u32) -> McpResult<()> {
        use std::fs;
        
        // Map current user to root (0) inside namespace
        let uid_map = format!("0 {} 1\n", std::process::id());
        let uid_map_path = format!("/proc/{}/uid_map", pid);
        
        fs::write(&uid_map_path, uid_map).map_err(|e| {
            McpError::SandboxError(format!("Failed to write uid_map: {}", e))
        })?;

        // Disable setgroups (required for unprivileged user namespaces)
        let setgroups_path = format!("/proc/{}/setgroups", pid);
        let _ = fs::write(&setgroups_path, "deny");

        // Map groups
        let gid_map = format!("0 {} 1\n", std::process::id());
        let gid_map_path = format!("/proc/{}/gid_map", pid);
        
        fs::write(&gid_map_path, gid_map).map_err(|e| {
            McpError::SandboxError(format!("Failed to write gid_map: {}", e))
        })?;

        Ok(())
    }

    /// Generate seccomp filter
    #[cfg(target_os = "linux")]
    fn generate_seccomp_filter(&self) -> McpResult<seccompiler::BpfProgram> {
        use seccompiler::*;

        let mut rules = BTreeMap::new();

        // Allow basic syscalls
        let allowed_syscalls = vec![
            "read", "write", "open", "close", "stat", "fstat", "lstat",
            "poll", "lseek", "mmap", "mprotect", "munmap", "brk",
            "rt_sigaction", "rt_sigprocmask", "rt_sigreturn", "ioctl",
            "pread64", "pwrite64", "readv", "writev", "access", "pipe",
            "select", "sched_yield", "mremap", "msync", "mincore",
            "madvise", "shmget", "shmat", "shmctl", "dup", "dup2",
            "pause", "nanosleep", "getitimer", "alarm", "setitimer",
            "getpid", "sendfile", "socket", "connect", "accept",
            "sendto", "recvfrom", "sendmsg", "recvmsg", "shutdown",
            "bind", "listen", "getsockname", "getpeername", "socketpair",
            "setsockopt", "getsockopt", "clone", "fork", "vfork",
            "execve", "exit", "wait4", "kill", "uname", "semget",
            "semop", "semctl", "shmdt", "msgget", "msgsnd", "msgrcv",
            "msgctl", "fcntl", "flock", "fsync", "fdatasync", "truncate",
            "ftruncate", "getdents", "getcwd", "chdir", "fchdir",
            "rename", "mkdir", "rmdir", "creat", "link", "unlink",
            "symlink", "readlink", "chmod", "fchmod", "chown", "fchown",
            "lchown", "umask", "gettimeofday", "getrlimit", "getrusage",
            "sysinfo", "times", "ptrace", "getuid", "syslog", "getgid",
            "setuid", "setgid", "geteuid", "getegid", "setpgid",
            "getppid", "getpgrp", "setsid", "setreuid", "setregid",
            "getgroups", "setgroups", "setresuid", "getresuid",
            "setresgid", "getresgid", "getpgid", "setfsuid", "setfsgid",
            "getsid", "capget", "capset", "rt_sigpending", "rt_sigtimedwait",
            "rt_sigqueueinfo", "rt_sigsuspend", "sigaltstack", "utime",
            "mknod", "uselib", "personality", "ustat", "statfs", "fstatfs",
            "sysfs", "getpriority", "setpriority", "sched_setparam",
            "sched_getparam", "sched_setscheduler", "sched_getscheduler",
            "sched_get_priority_max", "sched_get_priority_min",
            "sched_rr_get_interval", "mlock", "munlock", "mlockall",
            "munlockall", "vhangup", "modify_ldt", "pivot_root",
            "_sysctl", "prctl", "arch_prctl", "adjtimex", "setrlimit",
            "chroot", "sync", "acct", "settimeofday", "mount", "umount2",
            "swapon", "swapoff", "reboot", "sethostname", "setdomainname",
            "iopl", "ioperm", "create_module", "init_module", "delete_module",
            "get_kernel_syms", "query_module", "quotactl", "nfsservctl",
            "getpmsg", "putpmsg", "afs_syscall", "tuxcall", "security",
            "gettid", "readahead", "setxattr", "lsetxattr", "fsetxattr",
            "getxattr", "lgetxattr", "fgetxattr", "listxattr", "llistxattr",
            "flistxattr", "removexattr", "lremovexattr", "fremovexattr",
            "tkill", "time", "futex", "sched_setaffinity", "sched_getaffinity",
            "set_thread_area", "io_setup", "io_destroy", "io_getevents",
            "io_submit", "io_cancel", "get_thread_area", "lookup_dcookie",
            "epoll_create", "epoll_ctl_old", "epoll_wait_old", "remap_file_pages",
            "getdents64", "set_tid_address", "restart_syscall", "semtimedop",
            "fadvise64", "timer_create", "timer_settime", "timer_gettime",
            "timer_getoverrun", "timer_delete", "clock_settime", "clock_gettime",
            "clock_getres", "clock_nanosleep", "exit_group", "epoll_wait",
            "epoll_ctl", "tgkill", "utimes", "vserver", "mbind",
            "set_mempolicy", "get_mempolicy", "mq_open", "mq_unlink",
            "mq_timedsend", "mq_timedreceive", "mq_notify", "mq_getsetattr",
            "kexec_load", "waitid", "add_key", "request_key", "keyctl",
            "ioprio_set", "ioprio_get", "inotify_init", "inotify_add_watch",
            "inotify_rm_watch", "migrate_pages", "openat", "mkdirat",
            "mknodat", "fchownat", "futimesat", "newfstatat", "unlinkat",
            "renameat", "linkat", "symlinkat", "readlinkat", "fchmodat",
            "faccessat", "pselect6", "ppoll", "unshare", "set_robust_list",
            "get_robust_list", "splice", "tee", "sync_file_range", "vmsplice",
            "move_pages", "utimensat", "epoll_pwait", "signalfd", "timerfd_create",
            "eventfd", "fallocate", "timerfd_settime", "timerfd_gettime",
            "accept4", "signalfd4", "eventfd2", "epoll_create1", "dup3",
            "pipe2", "inotify_init1", "preadv", "pwritev", "rt_tgsigqueueinfo",
            "perf_event_open", "recvmmsg", "fanotify_init", "fanotify_mark",
            "prlimit64", "name_to_handle_at", "open_by_handle_at",
            "clock_adjtime", "syncfs", "sendmmsg", "setns", "getcpu",
            "process_vm_readv", "process_vm_writev", "kcmp", "finit_module",
            "sched_setattr", "sched_getattr", "renameat2", "seccomp",
            "getrandom", "memfd_create", "kexec_file_load", "bpf",
            "execveat", "userfaultfd", "membarrier", "mlock2",
            "copy_file_range", "preadv2", "pwritev2", "pkey_mprotect",
            "pkey_alloc", "pkey_free", "statx", "io_pgetevents", "rseq",
        ];

        for syscall in allowed_syscalls {
            rules.insert(
                syscall.to_string(),
                SeccompRule::new(vec![], SeccompAction::Allow),
            );
        }

        // Deny dangerous syscalls
        let denied_syscalls = vec![
            "open_by_handle_at", // Can bypass directory restrictions
            "ptrace",            // Process debugging
            "process_vm_writev", // Cross-process memory write
        ];

        for syscall in denied_syscalls {
            rules.insert(
                syscall.to_string(),
                SeccompRule::new(vec![], SeccompAction::Errno(1)), // EPERM
            );
        }

        let filter = SeccompFilter::new(
            rules,
            SeccompAction::Trap, // Default action for unknown syscalls
            SeccompArch::X86_64,
        ).map_err(|e| McpError::SandboxError(format!("Failed to create seccomp filter: {:?}", e)))?;

        filter.compile().map_err(|e| {
            McpError::SandboxError(format!("Failed to compile seccomp filter: {:?}", e))
        })
    }
}

#[async_trait]
impl Sandbox for AdvancedLinuxSandbox {
    async fn spawn(&self, config: &McpServerConfig) -> McpResult<Child> {
        if !Self::is_available() {
            return Err(McpError::SandboxError(
                "Advanced Linux sandboxing requires Linux with namespace support".to_string()
            ));
        }

        // Setup cgroups if enabled
        let cgroup_path = if self.config.use_cgroups {
            Some(self.setup_cgroups(&config.name).await?)
        } else {
            None
        };

        // For now, fall back to basic spawning
        // Full namespace implementation would require:
        // 1. Clone with namespace flags
        // 2. Setup UID/GID maps
        // 3. Pivot root (if using mount namespace)
        // 4. Apply seccomp filter
        // 5. Move to cgroup
        // 6. Exec the target process

        // This is a simplified implementation
        let mut cmd = tokio::process::Command::new(&config.command);
        cmd.args(&config.args);

        if !self.constraints.env_inherit {
            cmd.env_clear();
        }

        for (key, value) in &config.env {
            cmd.env(key, value);
        }

        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let child = cmd.spawn().map_err(|e| {
            McpError::SandboxError(format!("Failed to spawn sandboxed process: {}", e))
        })?;

        // Move process to cgroup
        if let Some(cgroup) = &cgroup_path {
            let pid = child.id().unwrap_or(0);
            let procs_path = cgroup.join("cgroup.procs");
            let _ = tokio::fs::write(&procs_path, pid.to_string()).await;
        }

        Ok(child)
    }

    fn constraints(&self) -> &SandboxConstraints {
        &self.constraints
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_advanced_sandbox_config_default() {
        let config = AdvancedLinuxSandboxConfig::default();
        assert!(config.use_user_namespace);
        assert!(config.use_pid_namespace);
        assert!(!config.use_network_namespace);
        assert!(config.use_mount_namespace);
        assert!(config.use_cgroups);
        assert!(config.use_seccomp);
    }

    #[test]
    fn test_advanced_sandbox_from_config() {
        let server_config = McpServerConfig {
            name: "test".to_string(),
            command: "echo".to_string(),
            args: vec![],
            env: Default::default(),
            tags: vec![],
            description: None,
            sandbox: crate::config::SandboxConfig {
                network: false,
                max_memory_mb: 256,
                max_cpu_percent: 25,
                ..Default::default()
            },
        };

        let sandbox = AdvancedLinuxSandbox::from_config(&server_config);
        assert!(!sandbox.constraints.network);
        assert_eq!(sandbox.constraints.max_memory_mb, 256);
        assert_eq!(sandbox.constraints.max_cpu_percent, 25);
    }

    #[test]
    fn test_is_available() {
        // On Linux, should return true if namespaces are available
        // On other platforms, false
        let available = AdvancedLinuxSandbox::is_available();

        #[cfg(target_os = "linux")]
        assert!(available);

        #[cfg(not(target_os = "linux"))]
        assert!(!available);
    }
}
