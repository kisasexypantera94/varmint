use applevisor::prelude::*;
use std::{
    sync::mpsc::{SyncSender, sync_channel},
    thread,
};

enum KickerCommand {
    Kick,
    Register(VcpuHandle),
}

#[derive(Clone)]
pub struct Kicker {
    tx: SyncSender<KickerCommand>,
}

impl Kicker {
    pub fn spawn<'scope, 'env>(
        scope: &'scope thread::Scope<'scope, 'env>,
        vm: &'env VirtualMachineInstance<GicEnabled>,
        handles: Vec<VcpuHandle>,
    ) -> Kicker {
        let (tx, rx) = sync_channel(16);

        scope.spawn(move || {
            let mut handles = handles;
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    KickerCommand::Kick => vm.vcpus_exit(&handles).unwrap(),
                    KickerCommand::Register(handle) => handles.push(handle),
                }
            }
        });

        Kicker { tx }
    }

    pub fn kick(&self) {
        let _ = self.tx.try_send(KickerCommand::Kick);
    }

    pub fn register(&self, handle: VcpuHandle) {
        let _ = self.tx.send(KickerCommand::Register(handle));
    }
}
