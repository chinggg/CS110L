use crate::debugger_command::DebuggerCommand;
use crate::dwarf_data::{DwarfData, Error as DwarfError};
use crate::inferior::{Inferior, Status};
use rustyline::error::ReadlineError;
use rustyline::Editor;

fn parse_address(addr: &str) -> Option<u64> {
    let addr_without_0x = if addr.to_lowercase().starts_with("0x") {
        &addr[2..]
    } else {
        &addr
    };
    u64::from_str_radix(addr_without_0x, 16).ok()
}

pub struct Debugger {
    target: String,
    history_path: String,
    readline: Editor<()>,
    inferior: Option<Inferior>,
    debug_data: DwarfData,
    breaks: Vec<u64>,
}

impl Debugger {
    /// Initializes the debugger.
    pub fn new(target: &str) -> Debugger {
        let debug_data = match DwarfData::from_file(target) {
            Ok(val) => val,
            Err(DwarfError::ErrorOpeningFile) => {
                println!("Could not open file {}", target);
                std::process::exit(1);
            }
            Err(DwarfError::DwarfFormatError(err)) => {
                println!("Could not debugging symbols from {}: {:?}", target, err);
                std::process::exit(1);
            }
        };

        debug_data.print();

        let history_path = format!("{}/.deet_history", std::env::var("HOME").unwrap());
        let mut readline = Editor::<()>::new();
        // Attempt to load history from ~/.deet_history if it exists
        let _ = readline.load_history(&history_path);

        Debugger {
            target: target.to_string(),
            history_path,
            readline,
            inferior: None,
            debug_data,
            breaks: Vec::<u64>::new(),
        }
    }

    pub fn run(&mut self) {
        loop {
            match self.get_next_command() {
                DebuggerCommand::Run(args) => {
                    // If inferior is not None, can only be stopped, kill it
                    // Because normally exited process has been set to None
                    if let Some(inferior) = &mut self.inferior {
                        println!("Killing running process (pid={})", inferior.pid());
                        inferior.kill().unwrap();
                    }
                    if let Some(inferior) = Inferior::new(&self.target, &args, &self.breaks) {
                        // Create the inferior
                        self.inferior = Some(inferior);
                        // You may use self.inferior.as_mut().unwrap() to get a mutable reference
                        // to the Inferior object
                        let status = self.inferior.as_mut().unwrap().cont().unwrap();
                        match status {
                            Status::Exited(exit_code) => {
                                println!("Process exited with code {}", exit_code);
                                self.inferior = None
                            }
                            Status::Signaled(signal) => {
                                println!("Process exited by signal {}", signal);
                                self.inferior = None
                            }
                            Status::Stopped(signal, rip) => {
                                println!("Process stopped with signal {} at address 0x{:x}", signal, rip);
                                self.inferior.as_ref().unwrap().print_stop(&self.debug_data).unwrap();
                            }
                        }
                    } else {
                        println!("Error starting subprocess");
                    }
                }
                DebuggerCommand::Continue => match &mut self.inferior {
                    Some(inferior) => {
                        let status = inferior.cont().expect("Fail to continue inferior process");
                        match status {
                            Status::Exited(exit_code) => {
                                println!("Process exited with code {}", exit_code);
                                self.inferior = None
                            }
                            Status::Signaled(signal) => {
                                println!("Process exited by signal {}", signal);
                                self.inferior = None
                            }
                            Status::Stopped(signal, rip) => {
                                println!("Process stopped with signal {} at address 0x{:x}", signal, rip);
                                self.inferior.as_ref().unwrap().print_stop(&self.debug_data).unwrap();
                            }
                        }
                    }
                    None => println!("No inferior process to continue"),
                },
                DebuggerCommand::BackTrace => match &self.inferior {
                    Some(inferior) => {
                        inferior.print_backtrace(&self.debug_data).unwrap();
                    }
                    None => println!("No inferior process to backtrace"),
                },
                DebuggerCommand::Quit => {
                    if let Some(inferior) = &mut self.inferior {
                        // inferior is not None, must be stopped
                        println!("Killing running process (pid={})", inferior.pid());
                        inferior.kill().unwrap();
                    }
                    return;
                }
                DebuggerCommand::BreakPoint(arg) => {
                    let mut break_addr: Option<u64> = None;
                    match arg.chars().next() {
                        Some('*') => {  // raw address
                            match parse_address(&arg[1..]) {
                                Some(addr) => {
                                    break_addr = Some(addr);
                                }
                                None => println!("Fail to parse address {}", &arg[1..]),
                            }
                        }
                        _ => {
                            match usize::from_str_radix(&arg, 10) {
                                Ok(line_number) => {
                                    match self.debug_data.get_addr_for_line(None, line_number) {
                                        Some(addr) => {  // debug_data may give wrong addr
                                            break_addr = Some(addr as u64);
                                        },
                                        None => println!("No address found for line {}", line_number),
                                    }
                                },
                                Err(_) => {  // function name
                                    match self.debug_data.get_addr_for_function(None, &arg){
                                        Some(addr) => {
                                            break_addr = Some(addr as u64);
                                        },
                                        None => println!("No address found for function {}", arg),
                                    }
                                }
                            }
                        }
                    }
                    if let Some(addr) = break_addr {
                        println!("Set breakpoint {} at address {:#x}", self.breaks.len(), addr);
                        self.breaks.push(addr);
                        if let Some(inferior) = &mut self.inferior {
                            let orig_byte = inferior.write_byte(addr, 0xcc).unwrap();
                            inferior.bp_map.insert(addr, orig_byte);
                        }
                    }
                }
            }
        }
    }

    /// This function prompts the user to enter a command, and continues re-prompting until the user
    /// enters a valid command. It uses DebuggerCommand::from_tokens to do the command parsing.
    ///
    /// You don't need to read, understand, or modify this function.
    fn get_next_command(&mut self) -> DebuggerCommand {
        loop {
            // Print prompt and get next line of user input
            match self.readline.readline("(deet) ") {
                Err(ReadlineError::Interrupted) => {
                    // User pressed ctrl+c. We're going to ignore it
                    println!("Type \"quit\" to exit");
                }
                Err(ReadlineError::Eof) => {
                    // User pressed ctrl+d, which is the equivalent of "quit" for our purposes
                    return DebuggerCommand::Quit;
                }
                Err(err) => {
                    panic!("Unexpected I/O error: {:?}", err);
                }
                Ok(line) => {
                    if line.trim().len() == 0 {
                        continue;
                    }
                    self.readline.add_history_entry(line.as_str());
                    if let Err(err) = self.readline.save_history(&self.history_path) {
                        println!(
                            "Warning: failed to save history file at {}: {}",
                            self.history_path, err
                        );
                    }
                    let tokens: Vec<&str> = line.split_whitespace().collect();
                    if let Some(cmd) = DebuggerCommand::from_tokens(&tokens) {
                        return cmd;
                    } else {
                        println!("Unrecognized command.");
                    }
                }
            }
        }
    }
}
