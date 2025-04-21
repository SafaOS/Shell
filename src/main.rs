const MULTI_PATH_SEP: &str = if cfg!(any(target_os = "windows", target_os = "safaos")) {
    ";"
} else {
    ":"
};

use std::{
    fmt::Display,
    io::{self, Write},
    path::Path,
    process::{Command, ExitStatus},
};

use lexer::Lexer;
use thiserror::Error;
mod builtin;
mod lexer;
use cfg_if::cfg_if;

// There is kinda of no need to use this, but it's nice to have
// support on both the host system and SafaOS
cfg_if! {
    if #[cfg(target_os = "safaos")] {
        use safa_api::errors::ErrorStatus;
        pub enum OSError {
            Known(ErrorStatus),
            Unknown(usize),
        }

        impl From<ExitStatus> for OSError {
            fn from(status: ExitStatus) -> Self {
                assert!(!status.success());
                let code = status.code().unwrap_or(1);
                if code > u16::MAX as i32 {
                    return OSError::Unknown(code as usize);
                }

                let error_status = ErrorStatus::try_from(code as u16);
                match error_status {
                    Ok(err) => OSError::Known(err),
                    Err(()) => OSError::Unknown(code as usize),
                }
            }
        }

        impl From<io::Error> for OSError {
            fn from(err: io::Error) -> Self {
                OSError::Known(safa_api::errors::err_from_io_error_kind(err.kind()))
            }
        }
    } else {
        use std::convert::Infallible;
        pub enum OSError {
            Known(Infallible),
            Unknown(usize),
        }

        impl From<ExitStatus> for OSError {
            fn from(status: ExitStatus) -> Self {
                assert!(!status.success());
                let code = status.code().unwrap_or(1);
                OSError::Unknown(code as usize)
            }
        }

        impl From<io::Error> for OSError {
            fn from(err: io::Error) -> Self {
                OSError::Unknown(1)
            }
        }
    }
}

impl Display for OSError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OSError::Known(err) => write!(f, "{err:?}"),
            OSError::Unknown(code) => write!(f, "{code}"),
        }
    }
}

struct Shell {
    stdin: io::Stdin,
    stdout: io::Stdout,
    last_command_failure: Option<OSError>,
}

#[derive(Debug, Error)]
pub enum ShellError {
    #[error("Failed with an IO error: {0}")]
    IoError(#[from] io::Error),
    #[error("Exited with status {0}")]
    ExitError(ExitStatus),
    // TODO: handle this better
    #[error("Builtin error")]
    BuiltinError,
}

impl From<ShellError> for OSError {
    fn from(err: ShellError) -> Self {
        match err {
            ShellError::IoError(err) => OSError::from(err),
            ShellError::ExitError(status) => OSError::from(status),
            ShellError::BuiltinError => OSError::Unknown(1),
        }
    }
}

impl Shell {
    fn new() -> Shell {
        Shell {
            stdin: io::stdin(),
            stdout: io::stdout(),
            last_command_failure: None,
        }
    }

    fn prompt(&mut self) -> String {
        let cwd = std::env::current_dir().expect("Failed to get current directory");

        print!("\x1b[35m{}\x1b[0m ", cwd.display());
        if let Some(err) = &self.last_command_failure {
            print!("\x1b[31m[{err}]\x1b[0m ");
        }
        print!("# ");

        self.stdout.flush().expect("Failed to flush stdout");

        let mut input = String::new();
        self.stdin
            .read_line(&mut input)
            .expect("Failed to read line from stdin");

        input
    }

    fn execute_program(&self, program: &str, args: &[&str]) -> Result<(), ShellError> {
        let path = std::env::var("PATH").expect("Failed to get the PATH Environment variable");
        let cwd = std::env::current_dir().expect("Failed to get CWD");

        let handle_child = |mut child: std::process::Child| {
            let results = child.wait()?;
            if !results.success() {
                Err(ShellError::ExitError(results))
            } else {
                Ok(())
            }
        };

        let path = path.split(MULTI_PATH_SEP);
        let path = path.map(|p| Path::new(p));
        let path = path.chain([cwd.as_path()].into_iter());

        for dir in path {
            let program_path = dir.join(program);
            if !program_path.exists() {
                continue;
            }

            let command = Command::new(program_path).args(args).spawn();
            match command {
                Ok(child) => return handle_child(child),
                Err(err) => return Err(ShellError::IoError(err)),
            }
        }

        let command = Command::new(program).args(args).spawn()?;
        handle_child(command)
    }

    fn execute(&mut self, input: &str) -> Result<(), ShellError> {
        let mut command = Lexer::new(input).map(|token| token.as_str());
        let Some(program) = command.next() else {
            return Ok(());
        };
        let program = program.as_ref();

        let args = command.collect::<Vec<_>>();
        let args = args.iter().map(|t| t.as_ref()).collect::<Vec<_>>();

        if let Some(f) = builtin::BUILTIN_COMMANDS.get(program) {
            return f(self, &args);
        }

        self.execute_program(program, &args)
    }

    fn run(mut self) {
        loop {
            let input = self.prompt();
            if let Err(err) = self.execute(&input) {
                if !matches!(err, ShellError::ExitError(_))
                    && !matches!(err, ShellError::BuiltinError)
                {
                    println!("Shell: {err}");
                }

                self.last_command_failure = Some(err.into());
            } else {
                self.last_command_failure = None;
            }
        }
    }
}

fn main() -> Result<(), ()> {
    unsafe {
        std::env::set_var("SHELL", "sys:/bin/safa");
    }

    let mut args = std::env::args();
    let program = args.next().expect("no program name passed");

    let mut interactive = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-i" | "--interactive" => interactive = true,
            "-c" => {
                let Some(command) = args.next() else {
                    println!("{program}: `-c` expected command");
                    return Err(());
                };

                let mut shell = Shell::new();
                return if let Err(err) = shell.execute(command.as_str()) {
                    println!("{program}: {err}");
                    Err(())
                } else {
                    Ok(())
                };
            }
            "--help" => {
                println!("usage: {program} [-i|--interactive|-c [command]]");
                return Ok(());
            }
            el => {
                println!("{program}: unexpected argument `{el}`");
                println!("usage: {program} [-i|--interactive|-c [command]]");
                return Err(());
            }
        }
    }

    if interactive {
        print!("\x1B[38;2;255;192;203m");
        print!(
            r#"
 ,---.             ,---.           ,-----.   ,---.
'   .-'   ,--,--. /  .-'  ,--,--. '  .-.  ' '   .-'
`.  `-.  ' ,-.  | |  `-, ' ,-.  | |  | |  | `.  `-.
.-'    | \ '-'  | |  .-' \ '-'  | '  '-'  ' .-'    |
`-----'   `--`--' `--'    `--`--'  `-----'  `-----'
        "#,
        );

        print!("\x1B[38;2;200;200;200m");
        print!(
            r#"
| Welcome to SafaOS!
| you are currently in ram:/, a playground
| init ramdisk has been mounted at sys:/
| sys:/bin is available in your PATH check it out for some binaries
| the command `help` will provide a list of builtin commands and some terminal usage guide
        "#
        );

        println!("\x1B[0m");
    }

    let shell = Shell::new();
    shell.run();
    Ok(())
}
