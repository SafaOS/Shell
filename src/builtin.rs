//! this module contains all the builtin shell commands

use std::io::Write;

use crate::{Shell, ShellError};
use phf::phf_map;

type BuiltInCommand = fn(&mut Shell, &[&str]) -> Result<(), ShellError>;
type BuiltInCommandList = phf::Map<&'static str, BuiltInCommand>;

pub static BUILTIN_COMMANDS: BuiltInCommandList = phf_map! {
    "exit" => |_, _| std::process::exit(0),
    "clear" => |_, _| {
        print!("\x1b[2J\x1b[H");
        std::io::stdout().flush()?;
        Ok(())
    },
    "cd" => |_, args| if args.len() < 1 {
        println!("cd: Not enough arguments");
        Err(ShellError::BuiltinError)
    } else {
        std::env::set_current_dir(args[0])?;
        Ok(())
    },
};
