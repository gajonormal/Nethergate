//! Samples only the Java PID owned by this hub. CPU is normalized to total CPU capacity.
use serde::Serialize;
use std::{
    collections::HashMap,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
#[derive(Default)]
pub struct Sampler {
    samples: HashMap<String, Sample>,
}
struct Sample {
    pid: u32,
    creation: u64,
    cpu: u64,
    at: Instant,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    supported: bool,
    pid: Option<u32>,
    cpu_percent: Option<f64>,
    memory_bytes: Option<u64>,
    private_bytes: Option<u64>,
    measured_at: u128,
    error: Option<String>,
}
struct Native {
    creation: u64,
    cpu: u64,
    memory: u64,
    private: u64,
}
fn cpu(previous: &Sample, pid: u32, current: &Native, at: Instant, cores: usize) -> Option<f64> {
    let elapsed = at.checked_duration_since(previous.at)?.as_secs_f64();
    if previous.pid != pid
        || previous.creation != current.creation
        || current.cpu < previous.cpu
        || elapsed < 0.001
    {
        return None;
    }
    Some(
        (((current.cpu - previous.cpu) as f64 / 10_000_000.0) / elapsed / cores.max(1) as f64
            * 100.0)
            .clamp(0.0, 100.0),
    )
}
impl Sampler {
    pub fn measure(&mut self, id: &str, pid: Option<u32>) -> Metrics {
        let mut view = Metrics {
            supported: cfg!(windows),
            pid,
            cpu_percent: None,
            memory_bytes: None,
            private_bytes: None,
            measured_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            error: None,
        };
        let Some(pid) = pid else {
            self.samples.remove(id);
            return view;
        };
        match native(pid) {
            Ok(sample) => {
                let at = Instant::now();
                view.cpu_percent = self.samples.get(id).and_then(|old| {
                    cpu(
                        old,
                        pid,
                        &sample,
                        at,
                        std::thread::available_parallelism()
                            .map(usize::from)
                            .unwrap_or(1),
                    )
                });
                view.memory_bytes = Some(sample.memory);
                view.private_bytes = Some(sample.private);
                self.samples.insert(
                    id.into(),
                    Sample {
                        pid,
                        creation: sample.creation,
                        cpu: sample.cpu,
                        at,
                    },
                );
            }
            Err(error) => {
                self.samples.remove(id);
                view.error = Some(error.to_string());
            }
        }
        view
    }
}
#[cfg(windows)]
fn native(pid: u32) -> anyhow::Result<Native> {
    use windows_sys::Win32::{
        Foundation::*,
        System::{ProcessStatus::*, Threading::*},
    };
    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    let handle =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid) };
    if handle.is_null() {
        anyhow::bail!("A amostra do processo Java não está disponível");
    }
    let handle = Handle(handle);
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let mut memory: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
    memory.cb = std::mem::size_of_val(&memory) as u32;
    if unsafe { GetProcessTimes(handle.0, &mut created, &mut exited, &mut kernel, &mut user) } == 0
        || unsafe {
            GetProcessMemoryInfo(
                handle.0,
                (&mut memory as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
                memory.cb,
            )
        } == 0
    {
        anyhow::bail!("Não foi possível ler a utilização de recursos do Java");
    }
    let ticks = |time: FILETIME| (time.dwHighDateTime as u64) << 32 | time.dwLowDateTime as u64;
    Ok(Native {
        creation: ticks(created),
        cpu: ticks(kernel).saturating_add(ticks(user)),
        memory: memory.WorkingSetSize as u64,
        private: memory.PrivateUsage as u64,
    })
}
#[cfg(not(windows))]
fn native(_pid: u32) -> anyhow::Result<Native> {
    anyhow::bail!("As métricas de processos estão disponíveis apenas no Windows nesta versão")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cpu_uses_elapsed_time_and_rejects_pid_reuse_or_counter_reset() {
        let at = Instant::now();
        let old = Sample {
            pid: 1,
            creation: 4,
            cpu: 0,
            at,
        };
        let mut sample = Native {
            creation: 4,
            cpu: 10_000_000,
            memory: 0,
            private: 0,
        };
        assert_eq!(
            cpu(&old, 1, &sample, at + std::time::Duration::from_secs(1), 4),
            Some(25.0)
        );
        assert!(cpu(&old, 2, &sample, at + std::time::Duration::from_secs(1), 4).is_none());
        sample.creation = 5;
        assert!(cpu(&old, 1, &sample, at + std::time::Duration::from_secs(1), 4).is_none());
    }
    #[test]
    #[cfg(windows)]
    fn reads_current_process_and_clears_samples_after_stop() {
        let mut sampler = Sampler::default();
        let view = sampler.measure("test", Some(std::process::id()));
        assert!(view.error.is_none());
        assert!(view.memory_bytes.unwrap() > 0);
        assert!(view.cpu_percent.is_none());
        let stopped = sampler.measure("test", None);
        assert!(stopped.memory_bytes.is_none());
        assert!(sampler.samples.is_empty());
    }
}
