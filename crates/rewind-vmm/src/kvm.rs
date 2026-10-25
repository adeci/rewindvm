//! Opening /dev/kvm, with what to do about it when that fails, and what
//! every new VM needs of KVM before its vCPU is made.

use anyhow::{Context, Result, bail};
use kvm_bindings::{KVM_CAP_PMU_CAPABILITY, KVM_PMU_CAP_DISABLE, kvm_enable_cap};
use kvm_ioctls::{Kvm, VmFd};

/// The device every VM is made through.
const DEVICE: &str = "/dev/kvm";

/// Where KVM says whether it gives VMs performance counters at all.
const ENABLE_PMU: &str = "/sys/module/kvm/parameters/enable_pmu";

/// /dev/kvm, opened, or why not and what would let it open.
pub fn open() -> Result<Kvm> {
    Kvm::new().map_err(|e| match hint(e.errno()) {
        Some(hint) => anyhow::anyhow!("opening {DEVICE}: {e}; {hint}"),
        None => anyhow::anyhow!("opening {DEVICE}: {e}"),
    })
}

/// What to do when opening /dev/kvm failed with `errno`, when anything
/// is known to help.
fn hint(errno: i32) -> Option<&'static str> {
    match errno {
        libc::ENOENT => Some(
            "this machine has no KVM, or its module is not loaded: try `sudo modprobe \
             kvm_intel` or `sudo modprobe kvm_amd`, turn virtualization on in the firmware, \
             or inside a VM turn on nested virtualization",
        ),
        libc::EACCES | libc::EPERM => Some(
            "add your user to the group /dev/kvm belongs to, which `ls -l /dev/kvm` names and \
             is most often kvm, then log in again",
        ),
        _ => None,
    }
}

/// What to do about a new VM's performance counters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Counters {
    /// Turn them off with KVM_CAP_PMU_CAPABILITY.
    Disable,
    /// KVM gives no VM counters on this host.
    Absent,
    /// This KVM gives VMs counters and cannot be asked not to.
    Stuck,
}

/// What to do about the counters, given the bits KVM_CAP_PMU_CAPABILITY
/// reports and whether KVM's `enable_pmu` parameter is on. KVM reports
/// no bits both when the parameter is off and when it predates the
/// capability, which Linux 6.2 added.
fn counters(capability: i32, enable_pmu: bool) -> Counters {
    if capability as u32 & KVM_PMU_CAP_DISABLE != 0 {
        return Counters::Disable;
    }
    if !enable_pmu {
        return Counters::Absent;
    }
    Counters::Stuck
}

/// Takes the VM's performance counters away before its vCPU is made. KVM
/// gives a VM counters whatever CPUID says, on AMD four of them, and they
/// count the host's cycles: a guest reading them, as a performance test
/// does through perf_event_open, reads a number no input of the run
/// decides. The monitor's own counters, which count the guest from the
/// host, are not the VM's and stay.
pub fn turn_off_counters(vm: &VmFd) -> Result<()> {
    let capability = vm.check_extension_raw(KVM_CAP_PMU_CAPABILITY.into());
    let enable_pmu = std::fs::read_to_string(ENABLE_PMU)
        .map(|v| v.trim() != "N")
        .unwrap_or(true);
    match counters(capability, enable_pmu) {
        Counters::Absent => Ok(()),
        Counters::Disable => {
            let cap = kvm_enable_cap {
                cap: KVM_CAP_PMU_CAPABILITY,
                args: [KVM_PMU_CAP_DISABLE.into(), 0, 0, 0],
                ..Default::default()
            };
            vm.enable_cap(&cap)
                .context("turning off the VM's performance counters")
        }
        Counters::Stuck => {
            bail!(
                "this host's KVM gives every VM the host's performance counters and cannot \
                 be asked not to, so runs would read host cycle counts; use Linux 6.2 or \
                 later, or load kvm with enable_pmu=0"
            )
        }
    }
}

#[cfg(test)]
mod tests {
    // The hint for each way opening /dev/kvm commonly fails, and what a
    // VM's counters need on each kind of host.
    use super::*;

    #[test]
    fn counters_are_turned_off_where_kvm_can_and_refused_where_it_cannot() {
        // A KVM that offers to disable the counters is asked to. One that
        // offers nothing, with enable_pmu off, gives no VM counters. One
        // that offers nothing with enable_pmu on predates the capability
        // and would hand the guest the host's counts, so it is refused.
        assert_eq!(
            counters(KVM_PMU_CAP_DISABLE as i32, true),
            Counters::Disable
        );
        assert_eq!(
            counters(KVM_PMU_CAP_DISABLE as i32, false),
            Counters::Disable
        );
        assert_eq!(counters(0, false), Counters::Absent);
        assert_eq!(counters(0, true), Counters::Stuck);
    }

    #[test]
    fn a_missing_or_forbidden_device_says_what_would_help() {
        // No device: KVM is off or its module is not loaded. Not allowed:
        // the group. Anything else has no hint.
        assert!(hint(libc::ENOENT).unwrap().contains("kvm_intel"));
        assert!(hint(libc::EACCES).unwrap().contains("group"));
        assert!(hint(libc::EPERM).unwrap().contains("group"));
        assert_eq!(hint(libc::EBUSY), None);
    }
}
