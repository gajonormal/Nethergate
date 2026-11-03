#[cfg(windows)]
use anyhow::bail;
use anyhow::Result;
use std::process::Child;

/// Closing the job kills SSH even if the desktop process exits unexpectedly.
pub(crate) struct OwnedChild {
    pub(crate) child: Child,
    #[cfg(windows)]
    _job: Job,
}
#[cfg(windows)]
struct Job(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
unsafe impl Send for Job {}
#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
impl OwnedChild {
    pub(crate) fn new(child: Child) -> Result<Self> {
        #[cfg(windows)]
        {
            let mut child = child;
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::System::JobObjects::*;
            let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if job.is_null() {
                let _ = child.kill();
                let _ = child.wait();
                bail!("Não foi possível proteger o processo do túnel");
            }
            let job = Job(job);
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if unsafe {
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of_val(&info) as u32,
                )
            } == 0
                || unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle()) } == 0
            {
                let _ = child.kill();
                let _ = child.wait();
                bail!("O Windows não permitiu supervisionar o túnel; a ligação foi cancelada");
            }
            Ok(Self { child, _job: job })
        }
        #[cfg(not(windows))]
        Ok(Self { child })
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
