use nix::sys::ptrace;
use nix::sys::signal;
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;
use std::collections::HashMap;
use std::mem::size_of;
use std::os::unix::process::CommandExt;
use std::process::Child;
use std::process::Command;

use crate::dwarf_data::DwarfData;

fn align_addr_to_word(addr: u64) -> u64 {
    addr & (-(size_of::<u64>() as i64) as u64)
}

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
    pub bp_map: HashMap<u64, u8>
}

impl Inferior {
    /// Attempts to start a new inferior process. Returns Some(Inferior) if successful, or None if
    /// an error is encountered.
    pub fn new(target: &str, args: &Vec<String>, breaks: &Vec<u64>) -> Option<Inferior> {
        let mut cmd = Command::new(target);
        unsafe {
            cmd.args(args).pre_exec(child_traceme);
        }
        let child = cmd.spawn().ok()?;
        let mut inferior = Inferior { child , bp_map: HashMap::<u64, u8>::new() };
        match inferior.wait(None) {
            Ok(Status::Stopped(signal::SIGTRAP, _)) => {
                for breakaddr in breaks {
                    let orig_byte = inferior.write_byte(*breakaddr, 0xcc).ok()?;
                    inferior.bp_map.insert(*breakaddr, orig_byte);
                }
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
    pub fn cont(&mut self) -> Result<Status, nix::Error> {
        let mut regs = ptrace::getregs(self.pid())?;
        let rip = regs.rip;
        if let Some(orig_byte) = self.bp_map.clone().get(&(rip - 1)) {  // double borrow if not clone
            self.write_byte(rip - 1, *orig_byte)?;
            regs.rip -= 1;
            ptrace::setregs(self.pid(), regs)?;
            ptrace::step(self.pid(), None)?;
            match self.wait(None) {
                Ok(Status::Stopped(signal::SIGTRAP, _addr)) => {
                    self.write_byte(rip - 1, 0xcc)?;
                }
                others => { return others; }
            }
        }
        ptrace::cont(self.pid(), None)?;
        self.wait(None)
    }

    // Kill stopped inferior and returns a Status to indicate the state of the process
    pub fn kill(&mut self) -> Result<Status, nix::Error> {
        self.child.kill().expect("Fail to kill inferior process");
        self.wait(None)
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

    pub fn print_stop(&self, debug_data: &DwarfData) -> Result<(), nix::Error> {
        let rip = ptrace::getregs(self.pid())?.rip as usize;
        let func = debug_data.get_function_from_addr(rip).unwrap();
        let line = debug_data.get_line_from_addr(rip).unwrap();
        println!("Stopped at {} ({})", func, line);
        Ok(())
    }

    pub fn write_byte(&mut self, addr: u64, val: u8) -> Result<u8, nix::Error> {
        let aligned_addr = align_addr_to_word(addr);
        let byte_offset = addr - aligned_addr;
        let word = ptrace::read(self.pid(), aligned_addr as ptrace::AddressType)? as u64;
        let orig_byte = (word >> 8 * byte_offset) & 0xff;
        let masked_word = word & !(0xff << 8 * byte_offset);
        let updated_word = masked_word | ((val as u64) << 8 * byte_offset);
        ptrace::write(
            self.pid(),
            aligned_addr as ptrace::AddressType,
            updated_word as *mut std::ffi::c_void,
        )?;
        Ok(orig_byte as u8)
    }
}
