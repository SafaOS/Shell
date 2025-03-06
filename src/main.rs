use std::{
    collections::HashMap,
    fmt::Display,
    io::{self, Write},
    path::Path,
    process::{Command, ExitStatus},
};

use thiserror::Error;
mod builtin;
use cfg_if::cfg_if;

// There is kinda of no need to use this, but it's nice to have
// support on both the host system and SafaOS
cfg_if! {
    if #[cfg(target_os = "safaos")] {
        use std::os::safaos::errors::ErrorStatus;
        pub enum OSError {
            Known(ErrorStatus),
            Unknown(usize),
        }

        impl From<ExitStatus> for OSError {
            fn from(status: ExitStatus) -> Self {
                assert!(!status.success());
                let code = status.code().unwrap_or(1);
                let error_status = ErrorStatus::try_from(code);
                match error_status {
                    Ok(err) => OSError::Known(err),
                    Err(()) => OSError::Unknown(code as usize),
                }
            }
        }

        impl From<io::Error> for OSError {
            fn from(err: io::Error) -> Self {
                OSError::Known(ErrorStatus::from_io_error_kind(err.kind()))
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
    // Env vars because SafaOS doesn't have them yet
    env: HashMap<String, String>,
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
            env: if cfg!(target_os = "safaos") {
                HashMap::from([(String::from("PATH"), String::from("sys:/bin"))])
            } else {
                HashMap::new()
            },
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
        let env_path = self
            .env
            .get("PATH")
            .expect("Failed to get the PATH Environment variable");

        let handle_child = |mut child: std::process::Child| {
            let results = child.wait()?;
            if !results.success() {
                Err(ShellError::ExitError(results))
            } else {
                Ok(())
            }
        };

        let path = env_path.split(';');

        for dir in path {
            let path = Path::new(dir);
            let program_path = path.join(program);
            // we currently have no way to check if the file exists so we have to do this:
            let command = Command::new(program_path).args(args).spawn();
            match command {
                Ok(child) => return handle_child(child),
                Err(err) => match err.kind() {
                    io::ErrorKind::NotFound | io::ErrorKind::IsADirectory => continue,
                    _ => return Err(ShellError::IoError(err)),
                },
            }
        }

        let command = Command::new(program).args(args).spawn()?;
        handle_child(command)
    }

    fn execute(&mut self, input: &str) -> Result<(), ShellError> {
        let mut command = input.split_whitespace();
        let Some(program) = command.next() else {
            return Ok(());
        };
        let args = command.collect::<Vec<_>>();

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

fn main() {
    let shell = Shell::new();
    shell.run();
}
