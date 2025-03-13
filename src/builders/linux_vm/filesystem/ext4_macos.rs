use crate::builders::linux_vm::LinuxVMBuildContext;
use crate::builders::Step;

pub struct EvaluateSize;

impl Step<LinuxVMBuildContext> for EvaluateSize {
    fn run(&mut self, _ctx: &mut LinuxVMBuildContext) -> anyhow::Result<()> {
        todo!("EXT4 support on MacOs")
    }
}

pub struct Format;

impl Step<LinuxVMBuildContext> for Format {
    fn run(&mut self, _ctx: &mut LinuxVMBuildContext) -> anyhow::Result<()> {
        todo!("EXT4 support on MacOs")
    }
}
