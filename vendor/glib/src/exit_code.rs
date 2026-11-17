// Take a look at the license at the top of the repository in the LICENSE file.

use std::process::Termination;

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub struct ExitCode(i32);

impl ExitCode {
    pub const SUCCESS: Self = Self(0);
    pub const FAILURE: Self = Self(1);

    pub fn value(&self) -> i32 {
        self.0
    }
}

impl From<i32> for ExitCode {
    fn from(value: i32) -> Self {
        Self(value)
    }
}

impl From<ExitCode> for i32 {
    fn from(value: ExitCode) -> Self {
        value.0
    }
}

impl Termination for ExitCode {
    fn report(self) -> std::process::ExitCode {
        std::process::ExitCode::from(self.0 as u8)
    }
}
