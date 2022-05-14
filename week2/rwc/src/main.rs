use std::fs::File;
use std::io::{BufRead, BufReader};
use std::{env, process};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Too few arguments.");
        process::exit(1);
    }
    let filename = &args[1];
    let file = File::open(filename).expect(format!("Could not open file {}", filename).as_str());
    let n_lines = BufReader::new(file).lines().count();
    println!("# of lines: {}", n_lines);
    // TODO: count words, characters
}
