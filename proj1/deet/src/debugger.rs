use crate::debugger_command::DebuggerCommand;
use crate::inferior::{Inferior, Status};
use rustyline::error::ReadlineError;
use rustyline::Editor;

pub struct Debugger {
    target: String,
    history_path: String,
    readline: Editor<()>,
    inferior: Option<Inferior>,
}

impl Debugger {
    /// Initializes the debugger.
    pub fn new(target: &str) -> Debugger {
        // TODO (milestone 3): initialize the DwarfData

        let history_path = format!("{}/.deet_history", std::env::var("HOME").unwrap());
        let mut readline = Editor::<()>::new();
        // Attempt to load history from ~/.deet_history if it exists
        let _ = readline.load_history(&history_path);

        Debugger {
            target: target.to_string(),
            history_path,
            readline,
            inferior: None,
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
                    if let Some(inferior) = Inferior::new(&self.target, &args) {
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
                            }
                        }
                    } else {
                        println!("Error starting subprocess");
                    }
                }
                DebuggerCommand::Continue => match &self.inferior {
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
                            }
                        }
                    }
                    None => println!("No inferior process to continue"),
                },
                DebuggerCommand::Quit => {
                    if let Some(inferior) = &mut self.inferior {
                        // inferior is not None, must be stopped
                        println!("Killing running process (pid={})", inferior.pid());
                        inferior.kill().unwrap();
                    }
                    return;
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
