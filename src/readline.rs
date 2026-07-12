use embedded_io::{ErrorType, Read as EmbRead, Write as EmbWrite};
use std::io::{Read, Stdin, Stdout, Write};

#[cfg(target_os = "safaos")]
pub fn enter_raw_mode() -> bool {
    const SET_FLAGS: u16 = 1;

    use safa_api::resource::Resource;
    use std::os::safaos::AsRawResource;

    unsafe {
        use std::mem::ManuallyDrop;

        let res = ManuallyDrop::new(Resource::from_raw(std::io::stdout().as_raw_resource()));
        res.io_command(SET_FLAGS, 0)
            .expect("Failed to set flags to raw mode");
        true
    }
}

#[cfg(not(target_os = "safaos"))]
pub fn enter_raw_mode() -> bool {
    use termion::raw::IntoRawMode;
    core::mem::forget(
        std::io::stdout()
            .into_raw_mode()
            .expect("Failed to turn stdout into raw mode"),
    );
    true
}

pub struct IOWrapper {
    pub stdin: Stdin,
    pub stdout: Stdout,
}

impl IOWrapper {
    pub fn new() -> Self {
        Self {
            stdin: std::io::stdin(),
            stdout: std::io::stdout(),
        }
    }
}

impl Default for IOWrapper {
    fn default() -> Self {
        Self::new()
    }
}

impl ErrorType for IOWrapper {
    type Error = embedded_io::ErrorKind;
}

impl EmbRead for IOWrapper {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        Ok(self.stdin.read(buf).map_err(|e| e.kind())?)
    }
}

impl EmbWrite for IOWrapper {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        Ok(self.stdout.write(buf).map_err(|e| e.kind())?)
    }
    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(self.stdout.flush().map_err(|e| e.kind())?)
    }
}
