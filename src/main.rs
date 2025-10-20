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
        pub enum OSReturn {
            Known(ErrorStatus),
            Unknown(isize),
        }

        impl From<ExitStatus> for OSReturn {
            fn from(status: ExitStatus) -> Self {
                let code = status.code().unwrap_or(0);
                if code.is_positive() || code == 0 || status.success() {
                    return OSReturn::Unknown(code as isize);
                }

                let error_status = ErrorStatus::try_from((-code) as u16);
                match error_status {
                    Ok(err) => OSReturn::Known(err),
                    Err(()) => OSReturn::Unknown(code as isize),
                }
            }
        }

        impl From<io::Error> for OSReturn {
            fn from(err: io::Error) -> Self {
                OSReturn::Known(safa_api::errors::err_from_io_error_kind(err.kind()))
            }
        }
    } else {
        use std::convert::Infallible;
        pub enum OSReturn {
            Known(Infallible),
            Unknown(usize),
        }

        impl From<ExitStatus> for OSReturn {
            fn from(status: ExitStatus) -> Self {
                assert!(!status.success());
                let code = status.code().unwrap_or(0);
                Self::Unknown(code as usize)
            }
        }

        impl From<io::Error> for OSReturn {
            fn from(err: io::Error) -> Self {
                Self::Unknown(1)
            }
        }
    }
}

impl Display for OSReturn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OSReturn::Known(err) => write!(f, "{err:?}"),
            OSReturn::Unknown(code) => write!(f, "{code}"),
        }
    }
}

struct Shell {
    stdin: io::Stdin,
    stdout: io::Stdout,
    last_command_return: Option<OSReturn>,
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

impl From<ShellError> for OSReturn {
    fn from(err: ShellError) -> Self {
        match err {
            ShellError::IoError(err) => OSReturn::from(err),
            ShellError::ExitError(status) => OSReturn::from(status),
            ShellError::BuiltinError => OSReturn::Unknown(-1),
        }
    }
}

impl Shell {
    fn new() -> Shell {
        Shell {
            stdin: io::stdin(),
            stdout: io::stdout(),
            last_command_return: None,
        }
    }

    fn prompt(&mut self) -> String {
        let cwd = std::env::current_dir().expect("Failed to get current directory");

        print!("\x1b[35m{}\x1b[0m ", cwd.display());
        if let Some(code) = &self.last_command_return {
            print!("\x1b[31m[{code}]\x1b[0m ");
        }
        print!("# ");

        self.stdout.flush().expect("Failed to flush stdout");

        let mut input = String::new();
        self.stdin
            .read_line(&mut input)
            .expect("Failed to read line from stdin");

        input
    }

    fn execute_program(&self, program: &str, args: &[&str]) -> Result<u32, ShellError> {
        let path = std::env::var("PATH").expect("Failed to get the PATH Environment variable");
        let cwd = std::env::current_dir().expect("Failed to get CWD");

        let handle_child = |mut child: std::process::Child| {
            let results = child.wait()?;
            if !results.success() {
                Err(ShellError::ExitError(results))
            } else {
                Ok(results.code().unwrap_or(0) as u32)
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

    fn execute(&mut self, input: &str) -> Result<u32, ShellError> {
        let mut command = Lexer::new(input).map(|token| token.as_str());
        let Some(program) = command.next() else {
            return Ok(0);
        };
        let program = program.as_ref();

        let args = command.collect::<Vec<_>>();
        let args = args.iter().map(|t| t.as_ref()).collect::<Vec<_>>();

        if let Some(f) = builtin::BUILTIN_COMMANDS.get(program) {
            return f(self, &args).map(|()| 0);
        }

        self.execute_program(program, &args)
    }

    fn run(mut self) {
        loop {
            let input = self.prompt();
            match self.execute(&input) {
                Err(err) => {
                    if !matches!(err, ShellError::ExitError(_))
                        && !matches!(err, ShellError::BuiltinError)
                    {
                        println!("Shell: {err}");
                    }

                    self.last_command_return = Some(err.into());
                }
                Ok(code) => {
                    self.last_command_return =
                        (code > 0).then_some(OSReturn::Unknown(code as isize));
                }
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
| to start GUI type opal-wm, ctrl+shift+T will open a terminal, to drag a window hold ctrl and the window.
        "#
        );

        println!("\x1B[0m");
    }

    let shell = Shell::new();
    shell.run();
    Ok(())
}
