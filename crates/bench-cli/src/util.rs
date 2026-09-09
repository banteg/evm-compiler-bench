use anyhow::{Context, Result, bail};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{IsTerminal, Read, Write},
    path::Path,
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::models::CommandStats;

pub struct MeasuredOutput {
    pub output: Output,
    pub stats: CommandStats,
}

pub fn run_measured(command: &mut Command, stdin: Option<&[u8]>) -> Result<MeasuredOutput> {
    let start = Instant::now();
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().context("spawning command")?;
    let stdout = child.stdout.take().context("opening child stdout")?;
    let stderr = child.stderr.take().context("opening child stderr")?;
    let stdout_handle = read_pipe(stdout);
    let stderr_handle = read_pipe(stderr);
    if let Some(input) = stdin {
        child
            .stdin
            .as_mut()
            .context("opening child stdin")?
            .write_all(input)?;
    }
    drop(child.stdin.take());
    let (status, usage) = wait_with_usage(child)?;
    let output = Output {
        status,
        stdout: stdout_handle
            .join()
            .map_err(|_| anyhow::anyhow!("stdout reader thread panicked"))??,
        stderr: stderr_handle
            .join()
            .map_err(|_| anyhow::anyhow!("stderr reader thread panicked"))??,
    };
    let wall_ms = start.elapsed().as_secs_f64() * 1000.0;
    Ok(MeasuredOutput {
        output,
        stats: CommandStats {
            wall_ms,
            cpu_ms: usage.cpu_ms,
            peak_rss_kib: usage.peak_rss_kib,
        },
    })
}

fn read_pipe<R>(mut pipe: R) -> thread::JoinHandle<Result<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut bytes = Vec::new();
        pipe.read_to_end(&mut bytes)?;
        Ok(bytes)
    })
}

pub fn require_success(measured: MeasuredOutput, label: &str) -> Result<MeasuredOutput> {
    if measured.output.status.success() {
        return Ok(measured);
    }
    bail!(
        "{label} failed with status {}\nstdout:\n{}\nstderr:\n{}",
        measured.output.status,
        String::from_utf8_lossy(&measured.output.stdout),
        String::from_utf8_lossy(&measured.output.stderr)
    );
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    Ok(sha256_bytes(&bytes))
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn strip_0x(value: &str) -> &str {
    value.strip_prefix("0x").unwrap_or(value)
}

pub fn byte_len(hex_value: &str) -> Result<usize> {
    Ok(hex::decode(strip_0x(hex_value))?.len())
}

pub fn stripped_cbor_len(hex_value: &str) -> Result<usize> {
    let bytes = hex::decode(strip_0x(hex_value))?;
    if bytes.len() < 2 {
        return Ok(bytes.len());
    }
    let metadata_len =
        u16::from_be_bytes([bytes[bytes.len() - 2], bytes[bytes.len() - 1]]) as usize;
    if metadata_len + 2 <= bytes.len() {
        let start = bytes.len() - metadata_len - 2;
        let metadata = &bytes[start..bytes.len() - 2];
        if looks_like_solc_or_vyper_metadata(metadata) {
            Ok(start)
        } else {
            Ok(bytes.len())
        }
    } else {
        Ok(bytes.len())
    }
}

fn looks_like_solc_or_vyper_metadata(metadata: &[u8]) -> bool {
    if metadata.is_empty() || !matches!(metadata[0], 0xa1..=0xbf) {
        return false;
    }
    metadata
        .windows(4)
        .any(|window| matches!(window, b"solc" | b"vyper" | b"ipfs" | b"bzzr"))
}

pub fn ensure_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("creating {}", path.display()))
}

pub struct Progress {
    label: String,
    total: usize,
    last_emit: Instant,
    started: Instant,
    emitted: bool,
    bar: Option<ProgressBar>,
}

impl Progress {
    pub fn new(label: impl Into<String>, total: usize) -> Self {
        let label = label.into();
        let tty = std::io::stderr().is_terminal();
        let bar = if tty {
            let bar = ProgressBar::new(total as u64);
            bar.set_draw_target(ProgressDrawTarget::stderr_with_hz(10));
            bar.set_style(
                ProgressStyle::with_template(
                    "{prefix:.bold}: {pos}/{len} ({percent:>3}%) {wide_msg} [{elapsed_precise}]",
                )
                .expect("progress style template is valid"),
            );
            bar.set_prefix(label.clone());
            Some(bar)
        } else {
            None
        };
        let mut progress = Self {
            label,
            total,
            last_emit: Instant::now(),
            started: Instant::now(),
            emitted: false,
            bar,
        };
        progress.emit(0, "starting");
        progress
    }

    pub fn update(&mut self, completed: usize, detail: impl AsRef<str>) {
        if self.should_emit(completed) {
            self.emit(completed, detail.as_ref());
        }
    }

    pub fn finish(&mut self, detail: impl AsRef<str>) {
        if let Some(bar) = &self.bar {
            bar.set_position(self.total as u64);
            bar.set_message(detail.as_ref().to_string());
            bar.finish_and_clear();
            let elapsed = self.started.elapsed().as_secs();
            eprintln!(
                "{}: {}/{} (100.0%) {} [{}s]",
                self.label,
                self.total,
                self.total,
                detail.as_ref(),
                elapsed
            );
            self.last_emit = Instant::now();
            self.emitted = true;
            return;
        }
        self.emit(self.total, detail.as_ref());
    }

    fn should_emit(&self, completed: usize) -> bool {
        self.bar.is_some()
            || !self.emitted
            || completed == self.total
            || self.last_emit.elapsed() >= Duration::from_secs(2)
    }

    fn emit(&mut self, completed: usize, detail: &str) {
        if let Some(bar) = &self.bar {
            bar.set_position(completed as u64);
            bar.set_message(detail.to_string());
            self.last_emit = Instant::now();
            self.emitted = true;
            return;
        }

        let percent = if self.total == 0 {
            100.0
        } else {
            (completed as f64 / self.total as f64) * 100.0
        };
        let elapsed = self.started.elapsed().as_secs();
        let line = format!(
            "{}: {}/{} ({percent:5.1}%) {} [{}s]",
            self.label, completed, self.total, detail, elapsed
        );
        eprintln!("{line}");
        self.last_emit = Instant::now();
        self.emitted = true;
    }
}

#[derive(Clone, Copy)]
struct Usage {
    cpu_ms: f64,
    peak_rss_kib: u64,
}

#[cfg(unix)]
fn wait_with_usage(mut child: std::process::Child) -> Result<(std::process::ExitStatus, Usage)> {
    use libc::{rusage, wait4};
    use std::os::unix::process::ExitStatusExt;

    let pid = child.id() as libc::pid_t;
    let mut status = 0;
    let mut raw = std::mem::MaybeUninit::<rusage>::zeroed();
    let waited = unsafe { wait4(pid, &mut status, 0, raw.as_mut_ptr()) };
    if waited < 0 {
        let output = child.wait()?;
        return Ok((
            output,
            Usage {
                cpu_ms: 0.0,
                peak_rss_kib: 0,
            },
        ));
    }
    let raw = unsafe { raw.assume_init() };
    let user_ms = raw.ru_utime.tv_sec as f64 * 1000.0 + raw.ru_utime.tv_usec as f64 / 1000.0;
    let sys_ms = raw.ru_stime.tv_sec as f64 * 1000.0 + raw.ru_stime.tv_usec as f64 / 1000.0;
    let peak = raw.ru_maxrss as u64;
    #[cfg(target_os = "macos")]
    let peak = peak / 1024;
    Ok((
        ExitStatusExt::from_raw(status),
        Usage {
            cpu_ms: user_ms + sys_ms,
            peak_rss_kib: peak,
        },
    ))
}

#[cfg(not(unix))]
fn wait_with_usage(mut child: std::process::Child) -> Result<(std::process::ExitStatus, Usage)> {
    let status = child.wait()?;
    Ok((
        status,
        Usage {
            cpu_ms: 0.0,
            peak_rss_kib: 0,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::stripped_cbor_len;

    #[test]
    fn strips_known_metadata_trailer() {
        assert_eq!(stripped_cbor_len("0x6000a164736f6c630006").unwrap(), 2);
    }

    #[test]
    fn keeps_plain_bytecode_with_trailing_length_like_bytes() {
        assert_eq!(stripped_cbor_len("0x6000aabb0002").unwrap(), 6);
    }
}
