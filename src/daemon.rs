use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use crate::cli::Cli;

/// Handle daemon mode initialization
pub fn handle_daemon_mode(cli: &Cli) -> Result<(), String> {
    #[cfg(unix)]
    {
        // Double fork to implement daemon
        match unsafe { libc::fork() } {
            -1 => {
                return Err("Failed to create child process".to_string());
            }
            0 => {
                // First child process
                if unsafe { libc::setsid() } == -1 {
                    return Err("Failed to create new session".to_string());
                }

                match unsafe { libc::fork() } {
                    -1 => {
                        return Err("Failed to create second child process".to_string());
                    }
                    0 => {
                        // Second child process (the actual daemon)
                        if let Err(e) = setup_daemon_stdio() {
                            eprintln!("Failed to setup daemon stdio: {}", e);
                            std::process::exit(1);
                        }

                        // Write PID file
                        if let Some(pid_file) = &cli.pid_file {
                            if let Err(e) = write_pid_file(pid_file) {
                                eprintln!("Failed to write PID file: {}", e);
                                std::process::exit(1);
                            }
                        }
                    }
                    _ => {
                        // First child process exits
                        std::process::exit(0);
                    }
                }
            }
            _ => {
                // Parent process exits
                std::process::exit(0);
            }
        }
    }

    #[cfg(not(unix))]
    {
        return Err("Daemon mode is only supported on Unix systems".to_string());
    }

    Ok(())
}

/// Setup standard I/O for daemon process
#[cfg(unix)]
fn setup_daemon_stdio() -> Result<(), String> {
    use std::os::fd::AsRawFd;

    // Redirect standard input/output to /dev/null
    let _ = File::create("/dev/null").map(|f| unsafe {
        let _ = libc::dup2(f.as_raw_fd(), libc::STDIN_FILENO);
        let _ = libc::dup2(f.as_raw_fd(), libc::STDOUT_FILENO);
        let _ = libc::dup2(f.as_raw_fd(), libc::STDERR_FILENO);
    });

    Ok(())
}

/// Write process ID to PID file
fn write_pid_file(pid_file: &PathBuf) -> Result<(), String> {
    let mut file = File::create(pid_file)
        .map_err(|e| format!("Failed to create PID file '{}': {}", pid_file.display(), e))?;

    writeln!(file, "{}", std::process::id())
        .map_err(|e| format!("Failed to write to PID file '{}': {}", pid_file.display(), e))?;

    Ok(())
}

/// Change working directory if specified
pub fn change_working_directory(work_dir: &Option<PathBuf>) -> Result<(), String> {
    if let Some(dir) = work_dir {
        std::env::set_current_dir(dir)
            .map_err(|e| format!("Failed to change working directory to '{}': {}", dir.display(), e))?;
    }
    Ok(())
}