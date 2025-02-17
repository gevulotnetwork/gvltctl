use crate::builders::linux_vm::LinuxVMBuildContext;
use crate::builders::Step;

#[allow(dead_code)]
pub struct MountFileSystem(pub &'static str);

impl Step<LinuxVMBuildContext> for MountFileSystem {
    fn run(&mut self, _ctx: &mut LinuxVMBuildContext) -> anyhow::Result<()> {
        todo!("FUSE support on MacOS")
    }
}
