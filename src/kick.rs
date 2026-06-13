use applevisor::prelude::*;
use std::{
    sync::mpsc::{SyncSender, sync_channel},
    thread,
};

#[derive(Clone)]
pub struct Kicker {
    tx: SyncSender<()>,
}

impl Kicker {
    pub fn spawn<'scope, 'env>(
        scope: &'scope thread::Scope<'scope, 'env>,
        vm: &'env VirtualMachineInstance<GicEnabled>,
        handle: VcpuHandle,
    ) -> Kicker {
        let (tx, rx) = sync_channel(1);

        scope.spawn(move || {
            while rx.recv().is_ok() {
                vm.vcpus_exit(&[handle.clone()]).unwrap();
            }
        });

        Kicker { tx }
    }

    pub fn kick(&self) {
        let _ = self.tx.try_send(());
    }
}
