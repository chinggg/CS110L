use nix::sys::ptrace;
use nix::sys::signal;
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;
use std::os::unix::process::CommandExt;
use std::process::Child;
use std::process::Command;

use crate::dwarf_data::DwarfData;

#[derive(Debug)]
pub enum Status {
    /// Indicates inferior stopped. Contains the signal that stopped the process, as well as the
    /// current instruction pointer that it is stopped at.
    Stopped(signal::Signal, usize),

    /// Indicates inferior exited normally. Contains the exit status code.
    Exited(i32),

    /// Indicates the inferior exited due to a signal. Contains the signal that killed the
    /// process.
    Signaled(signal::Signal),
}

/// This function calls ptrace with PTRACE_TRACEME to enable debugging on a process. You should use
/// pre_exec with Command to call this in the child process.
fn child_traceme() -> Result<(), std::io::Error> {
    ptrace::traceme().or(Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "ptrace TRACEME failed",
    )))
}

#[derive(Debug)]
pub struct Inferior {
    child: Child,
}

impl Inferior {
    /// Attempts to start a new inferior process. Returns Some(Inferior) if successful, or None if
    /// an error is encountered.
    pub fn new(target: &str, args: &Vec<String>) -> Option<Inferior> {
        let mut cmd = Command::new(target);
        unsafe {
            cmd.args(args).pre_exec(child_traceme);
        }
        let child = cmd.spawn().ok()?;
        let inferior = Inferior { child };
        match inferior.wait(None) {
            Ok(Status::Stopped(signal::SIGTRAP, _)) => {
                Some(inferior)
            }
            _ => None
        }
    }

    /// Returns the pid of this inferior.
    pub fn pid(&self) -> Pid {
        nix::unistd::Pid::from_raw(self.child.id() as i32)
    }

    /// Calls waitpid on this inferior and returns a Status to indicate the state of the process
    /// after the waitpid call.
    pub fn wait(&self, options: Option<WaitPidFlag>) -> Result<Status, nix::Error> {
        Ok(match waitpid(self.pid(), options)? {
            WaitStatus::Exited(_pid, exit_code) => Status::Exited(exit_code),
            WaitStatus::Signaled(_pid, signal, _core_dumped) => Status::Signaled(signal),
            WaitStatus::Stopped(_pid, signal) => {
                let regs = ptrace::getregs(self.pid())?;
                Status::Stopped(signal, regs.rip as usize)
            }
            other => panic!("waitpid returned unexpected status: {:?}", other),
        })
    }

    // Continue stopped inferior and returns a Status to indicate the state of the process
    pub fn cont(&self) -> Result<Status, nix::Error> {
        ptrace::cont(self.pid(), None)?;
        self.wait(None)
    }

    // Kill stopped inferior and returns a Status to indicate the state of the process
    pub fn kill(&mut self) -> Result<Status, nix::Error> {
        self.child.kill().expect("Fail to kill inferior process");
        self.wait(None)
    }

    pub fn print_stop(&self, debug_data: &DwarfData) -> Result<(), nix::Error> {
        let rip = ptrace::getregs(self.pid())?.rip as usize;
        let func = debug_data.get_function_from_addr(rip).unwrap();
        let line = debug_data.get_line_from_addr(rip).unwrap();
        println!("Stopped at {} ({})", func, line);
        Ok(())
    }
    pub fn print_backtrace(&self, debug_data: &DwarfData) -> Result<(), nix::Error> {
        let regs = ptrace::getregs(self.pid())?;
        let mut rip = regs.rip as usize;
        let mut rbp = regs.rbp as usize;
        println!("%rip register: {:#x}", rip);
        loop {
          let func = debug_data.get_function_from_addr(rip).unwrap();
          let line = debug_data.get_line_from_addr(rip).unwrap();
          println!("{} ({})", func, line);
          if func == "main" {
              break;
          }
          rip = ptrace::read(self.pid(), (rbp + 8) as ptrace::AddressType)? as usize;
          rbp = ptrace::read(self.pid(), rbp as ptrace::AddressType)? as usize;
        }
        Ok(())
    }
}
