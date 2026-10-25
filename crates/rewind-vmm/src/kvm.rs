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

/// The first Linux release whose KVM knows KVM_CAP_PMU_CAPABILITY.
const PMU_CAPABILITY_SINCE: (u32, u32) = (6, 2);

/// Whether this host's KVM knows KVM_CAP_PMU_CAPABILITY, by the kernel's
/// release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Capability {
    Known,
    Unknown,
}

impl Capability {
    /// What a kernel of this release, such as "6.18.44" or
    /// "7.1.2-zen1", knows. A release that does not parse is taken as an
    /// old one.
    fn of_release(release: &str) -> Capability {
        let mut parts = release
            .split(|c: char| !c.is_ascii_digit())
            .map(|p| p.parse::<u32>().ok());
        match (parts.next().flatten(), parts.next().flatten()) {
            (Some(major), Some(minor)) if (major, minor) >= PMU_CAPABILITY_SINCE => {
                Capability::Known
            }
            _ => Capability::Unknown,
        }
    }
}

/// What to do about the counters, given the bits KVM_CAP_PMU_CAPABILITY
/// reports, whether the kernel knows the capability, and KVM's
/// `enable_pmu` parameter when it can be read. A KVM that knows the
/// capability reports no bits only when it gives VMs no counters at all,
/// as when it runs in a VM itself without a PMU or `enable_pmu` is off.
/// An older one reports no bits whatever it gives, and only `enable_pmu`
/// off says it gives none; a build sandbox has no /sys to read it from.
fn counters(capability: i32, kernel: Capability, enable_pmu: Option<bool>) -> Counters {
    if capability as u32 & KVM_PMU_CAP_DISABLE != 0 {
        return Counters::Disable;
    }
    if kernel == Capability::Known || enable_pmu == Some(false) {
        return Counters::Absent;
    }
    Counters::Stuck
}

/// The running kernel's release, as uname gives it.
fn kernel_release() -> String {
    // SAFETY: utsname is plain bytes, for which zero is a value.
    let mut name: libc::utsname = unsafe { std::mem::zeroed() };
    // SAFETY: uname writes only into the utsname it is given.
    if unsafe { libc::uname(&mut name) } != 0 {
        return String::new();
    }
    // SAFETY: uname NUL-terminates the release.
    let release = unsafe { std::ffi::CStr::from_ptr(name.release.as_ptr()) };
    release.to_string_lossy().into_owned()
}

/// Takes the VM's performance counters away before its vCPU is made. KVM
/// gives a VM counters whatever CPUID says, on AMD four of them, and they
/// count the host's cycles: a guest reading them, as a performance test
/// does through perf_event_open, reads a number no input of the run
/// decides. The monitor's own counters, which count the guest from the
/// host, are not the VM's and stay.
pub fn turn_off_counters(vm: &VmFd) -> Result<()> {
    let capability = vm.check_extension_raw(KVM_CAP_PMU_CAPABILITY.into());
    let kernel = Capability::of_release(&kernel_release());
    let enable_pmu = std::fs::read_to_string(ENABLE_PMU)
        .ok()
        .map(|v| v.trim() != "N");
    match counters(capability, kernel, enable_pmu) {
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
                "this host's KVM may give every VM the host's performance counters, and \
                 cannot be asked not to, so runs could read host cycle counts; use Linux 6.2 \
                 or later, or load kvm with enable_pmu=0"
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
        // knows the capability and offers nothing gives VMs no counters,
        // as GitHub's runners, VMs without a PMU, do, whether or not /sys
        // can be read. An older one is taken at enable_pmu's word, and
        // refused when it is on or cannot be read, as in a build sandbox.
        use Capability::{Known, Unknown};
        let disable = KVM_PMU_CAP_DISABLE as i32;
        assert_eq!(counters(disable, Known, Some(true)), Counters::Disable);
        assert_eq!(counters(disable, Unknown, None), Counters::Disable);
        assert_eq!(counters(0, Known, None), Counters::Absent);
        assert_eq!(counters(0, Known, Some(true)), Counters::Absent);
        assert_eq!(counters(0, Unknown, Some(false)), Counters::Absent);
        assert_eq!(counters(0, Unknown, Some(true)), Counters::Stuck);
        assert_eq!(counters(0, Unknown, None), Counters::Stuck);
    }

    #[test]
    fn a_kernel_release_says_whether_it_knows_the_capability() {
        // 6.2 and later know it, whatever follows the version; 6.1, 5.15
        // and a release that does not parse do not.
        assert_eq!(Capability::of_release("6.2.0"), Capability::Known);
        assert_eq!(Capability::of_release("6.18.44"), Capability::Known);
        assert_eq!(Capability::of_release("7.1.2-zen1"), Capability::Known);
        assert_eq!(
            Capability::of_release("6.8.0-1017-azure"),
            Capability::Known
        );
        assert_eq!(Capability::of_release("6.1.90"), Capability::Unknown);
        assert_eq!(Capability::of_release("5.15.0"), Capability::Unknown);
        assert_eq!(Capability::of_release(""), Capability::Unknown);
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
