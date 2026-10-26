//! Counting the guest's work with the host's performance counters.
//!
//! Virtual time that moves only at exits makes computation free: a thread
//! that computes for a millisecond between two exits sees no time pass,
//! and neither does the scheduler. A hardware counter of retired branches,
//! restricted to guest mode, measures the work between two exits, and
//! because it is read only at exits, which are fixed points in the guest's
//! instruction stream, the count read there is the same on every run, as
//! long as the counter itself is exact. rr relies on the same property of
//! the same counter.

use std::io;

use anyhow::{Context, Result, bail};

use crate::CounterEvent;

/// perf_event_attr, as far as this monitor uses it.
#[repr(C)]
#[derive(Default)]
struct PerfEventAttr {
    type_: u32,
    size: u32,
    config: u64,
    sample_period: u64,
    sample_type: u64,
    read_format: u64,
    flags: u64,
    wakeup_events: u32,
    bp_type: u32,
    config1: u64,
    config2: u64,
    branch_sample_type: u64,
    sample_regs_user: u64,
    sample_stack_user: u32,
    clockid: i32,
    sample_regs_intr: u64,
    aux_watermark: u32,
    sample_max_stack: u16,
    reserved_2: u16,
    aux_sample_size: u32,
    reserved_3: u32,
    sig_data: u64,
    config3: u64,
}

const PERF_TYPE_HARDWARE: u32 = 0;
const PERF_TYPE_RAW: u32 = 4;
const PERF_COUNT_HW_INSTRUCTIONS: u64 = 1;

/// Flag bits in perf_event_attr.flags.
const FLAG_DISABLED: u64 = 1 << 0;
/// Always on the hardware while enabled: the kernel never takes a pinned
/// event off to share the counters, and puts one that cannot fit in an
/// error state, where a read gives nothing, rather than counting short.
const FLAG_PINNED: u64 = 1 << 2;
const FLAG_EXCLUDE_KERNEL: u64 = 1 << 5;
const FLAG_EXCLUDE_HV: u64 = 1 << 6;
const FLAG_EXCLUDE_HOST: u64 = 1 << 19;

/// Bits in perf_event_attr.read_format: a read gives, after the count,
/// how long the event was enabled and how long it was on the hardware.
const PERF_FORMAT_TOTAL_TIME_ENABLED: u64 = 1 << 0;
const PERF_FORMAT_TOTAL_TIME_RUNNING: u64 = 1 << 1;

const PERF_EVENT_IOC_ENABLE: u64 = 0x2400;
const PERF_EVENT_IOC_DISABLE: u64 = 0x2401;
const PERF_EVENT_IOC_REFRESH: u64 = 0x2402;
const PERF_EVENT_IOC_RESET: u64 = 0x2403;
/// _IOW('$', 4, u64)
const PERF_EVENT_IOC_PERIOD: u64 = 0x4008_2404;

/// struct f_owner_ex and the fcntl that takes it, from linux/fcntl.h, which
/// the libc crate does not carry.
#[repr(C)]
struct FOwnerEx {
    type_: i32,
    pid: i32,
}
const F_SETOWN_EX: i32 = 15;
const F_OWNER_TID: i32 = 0;
const F_SETSIG: i32 = 10;

/// The signal a counter overflow sends the vCPU thread. Any signal pulls
/// the thread out of KVM_RUN; this one is otherwise unused.
pub(crate) fn overflow_signal() -> i32 {
    libc::SIGRTMIN() + 3
}

/// Whether guest kernel mode is counted too. Counting it needs
/// perf_event_paranoid at 1 or lower, or CAP_PERFMON.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modes {
    UserOnly,
    UserAndKernel,
}

#[derive(Debug)]
pub struct Counter {
    fd: i32,
}

impl Counter {
    /// A counter of guest-mode events on the calling thread, which must be
    /// the vCPU's.
    pub fn open(event: CounterEvent, modes: Modes) -> Result<Counter> {
        let fd = open_event(event, modes, 0)?;
        // SAFETY: a counter fd we own.
        if unsafe { libc::ioctl(fd, PERF_EVENT_IOC_ENABLE as _, 0) } != 0 {
            bail!("enabling the counter: {}", io::Error::last_os_error());
        }
        Ok(Counter { fd })
    }

    pub fn read(&self) -> Result<u64> {
        read_count(self.fd)
    }

    /// Stops counting until `resume`. The counter counts the thread's
    /// guest, whichever machine's that is, so a machine that is not
    /// running pauses its counter while another runs on the thread.
    pub fn pause(&self) -> Result<()> {
        // SAFETY: an ioctl on an fd we own.
        if unsafe { libc::ioctl(self.fd, PERF_EVENT_IOC_DISABLE as _, 0) } != 0 {
            bail!("pausing the counter: {}", io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn resume(&self) -> Result<()> {
        // SAFETY: an ioctl on an fd we own.
        if unsafe { libc::ioctl(self.fd, PERF_EVENT_IOC_ENABLE as _, 0) } != 0 {
            bail!("resuming the counter: {}", io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for Counter {
    fn drop(&mut self) {
        // SAFETY: closing an fd we own.
        unsafe { libc::close(self.fd) };
    }
}

/// A second counter of the same event, used only for its overflow: armed
/// to overflow after a number of events, it signals the vCPU thread, which
/// pulls the vCPU out of KVM_RUN a little after that point.
#[derive(Debug)]
pub struct Overflow {
    fd: i32,
}

impl Overflow {
    /// Must be called on the vCPU's thread, which the signal is sent to.
    pub fn open(event: CounterEvent, modes: Modes) -> Result<Overflow> {
        install_signal_handler();
        // A period so large it never fires until armed.
        let fd = open_event(event, modes, 1 << 62)?;
        // SAFETY: fcntl on an fd we own, with the calling thread's id.
        unsafe {
            let owner = FOwnerEx {
                type_: F_OWNER_TID,
                pid: libc::gettid(),
            };
            if libc::fcntl(fd, F_SETOWN_EX, &owner) != 0
                || libc::fcntl(fd, F_SETSIG, overflow_signal()) != 0
                || libc::fcntl(fd, libc::F_SETFL, libc::O_ASYNC) != 0
            {
                bail!("routing counter overflows: {}", io::Error::last_os_error());
            }
        }
        Ok(Overflow { fd })
    }

    /// Signals the thread after `events` more events, once.
    pub fn arm(&self, events: u64) -> Result<()> {
        let events = events.max(1);
        // SAFETY: ioctls on an fd we own.
        unsafe {
            if libc::ioctl(self.fd, PERF_EVENT_IOC_DISABLE as _, 0) != 0
                || libc::ioctl(self.fd, PERF_EVENT_IOC_RESET as _, 0) != 0
                || libc::ioctl(self.fd, PERF_EVENT_IOC_PERIOD as _, &events) != 0
                || libc::ioctl(self.fd, PERF_EVENT_IOC_REFRESH as _, 1) != 0
            {
                bail!("arming the overflow: {}", io::Error::last_os_error());
            }
        }
        Ok(())
    }

    pub fn disarm(&self) -> Result<()> {
        // SAFETY: an ioctl on an fd we own.
        if unsafe { libc::ioctl(self.fd, PERF_EVENT_IOC_DISABLE as _, 0) } != 0 {
            bail!("disarming the overflow: {}", io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for Overflow {
    fn drop(&mut self) {
        // SAFETY: closing an fd we own.
        unsafe { libc::close(self.fd) };
    }
}

/// A handler that does nothing: the signal's only job is to interrupt
/// KVM_RUN, which a pending signal with a handler does.
pub(crate) fn install_signal_handler() {
    extern "C" fn nothing(_: i32) {}
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        // SAFETY: installing a handler that touches no state.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = nothing as *const () as usize;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(overflow_signal(), &action, std::ptr::null_mut());
        }
    });
}

/// What a read of a counter opened with [`PERF_FORMAT_TOTAL_TIME_ENABLED`]
/// and [`PERF_FORMAT_TOTAL_TIME_RUNNING`] gives.
#[repr(C)]
#[derive(Debug, Default)]
struct Reading {
    value: u64,
    enabled_ns: u64,
    running_ns: u64,
}

impl Reading {
    /// The count, when the counter was on the hardware the whole time it
    /// was enabled. The kernel takes a counter off the hardware to share
    /// it among more events than fit, and a count from part of the time
    /// is short by work the guest did, so a run read it would not replay.
    fn count(&self) -> Result<u64> {
        if self.running_ns != self.enabled_ns {
            bail!(
                "the host took the guest's counter off the hardware: it counted for {} of {} ns, \
                 so its count is short. Another program on the host is using the performance \
                 counters, as perf does; stop it, or run on another host",
                self.running_ns,
                self.enabled_ns
            );
        }
        Ok(self.value)
    }
}

fn read_count(fd: i32) -> Result<u64> {
    let mut reading = Reading::default();
    let len = size_of::<Reading>();
    // SAFETY: reading at most `len` bytes into a repr(C) struct of u64s.
    let n = unsafe { libc::read(fd, (&mut reading as *mut Reading).cast(), len) };

    // A pinned counter that could not get the hardware reads as end of
    // file.
    if n == 0 {
        bail!(
            "the host gave the guest's counter no place on the hardware: another program on the \
             host is using the performance counters, as perf does; stop it, or run on another host"
        );
    }
    if n != len as isize {
        return Err(io::Error::last_os_error()).context("reading the counter");
    }
    reading.count()
}

/// Opens a guest-mode counter on the calling thread, disabled; with a
/// sample period, it overflows every that many events.
fn open_event(event: CounterEvent, modes: Modes, sample_period: u64) -> Result<i32> {
    {
        let (type_, config) = match event {
            CounterEvent::AmdRetiredConditionalBranches => (PERF_TYPE_RAW, 0xd1),
            CounterEvent::IntelRetiredConditionalBranches => (PERF_TYPE_RAW, 0x01c4),
            CounterEvent::Instructions => (PERF_TYPE_HARDWARE, PERF_COUNT_HW_INSTRUCTIONS),
        };
        let mut flags = FLAG_DISABLED | FLAG_PINNED | FLAG_EXCLUDE_HV | FLAG_EXCLUDE_HOST;
        if modes == Modes::UserOnly {
            flags |= FLAG_EXCLUDE_KERNEL;
        }
        let attr = PerfEventAttr {
            type_,
            size: size_of::<PerfEventAttr>() as u32,
            config,
            flags,
            sample_period,
            read_format: PERF_FORMAT_TOTAL_TIME_ENABLED | PERF_FORMAT_TOTAL_TIME_RUNNING,
            ..Default::default()
        };
        // SAFETY: a valid attr for the duration of the call.
        let fd = unsafe {
            libc::syscall(
                libc::SYS_perf_event_open,
                &attr as *const PerfEventAttr,
                0,  // this thread
                -1, // any CPU
                -1, // no group
                0,
            )
        } as i32;
        if fd < 0 {
            let err = io::Error::last_os_error();
            bail!("perf_event_open for {event:?}: {err}; see /proc/sys/kernel/perf_event_paranoid");
        }
        Ok(fd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_count_off_the_hardware_part_of_the_time_is_refused() {
        // A reading whose counter ran the whole time it was enabled gives
        // its count, as does one never enabled. One the kernel took off
        // the hardware for part of that time, as it does to share the
        // counters among more events than fit, is short and is refused.
        let whole = Reading {
            value: 1234,
            enabled_ns: 5000,
            running_ns: 5000,
        };
        assert_eq!(whole.count().unwrap(), 1234);
        assert_eq!(Reading::default().count().unwrap(), 0);

        let short = Reading {
            value: 1000,
            enabled_ns: 5000,
            running_ns: 4000,
        };
        let err = short.count().unwrap_err().to_string();
        assert!(err.contains("4000 of 5000 ns"), "{err}");
    }
}
