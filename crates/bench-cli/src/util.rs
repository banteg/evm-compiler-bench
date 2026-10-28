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
        let written = child
            .stdin
            .take()
            .context("opening child stdin")?
            .write_all(input);
        // A child may exit without reading all of its input; its status and
        // output then explain the failure better than the broken pipe does.
        if let Err(error) = written
            && error.kind() != std::io::ErrorKind::BrokenPipe
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error).context("writing child stdin");
        }
    }
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
    let Some(suffix) = bytes.len().checked_sub(2) else {
        return Ok(bytes.len());
    };
    let declared = u16::from_be_bytes([bytes[suffix], bytes[suffix + 1]]) as usize;
    // solc and Vyper before 0.3.10 count only the CBOR payload; later Vyper
    // releases also count the two length bytes.
    let starts = [
        suffix.checked_sub(declared),
        bytes.len().checked_sub(declared),
    ];
    Ok(starts
        .into_iter()
        .flatten()
        .find(|&start| start <= suffix && looks_like_compiler_metadata(&bytes[start..suffix]))
        .unwrap_or(bytes.len()))
}

/// Keys of solc (`solc`, `ipfs`, `bzzr0`, `bzzr1`) and Vyper (`vyper`)
/// metadata, matched in their CBOR text-string encoding.
const METADATA_KEYS: [&str; 5] = ["solc", "vyper", "ipfs", "bzzr0", "bzzr1"];

fn looks_like_compiler_metadata(metadata: &[u8]) -> bool {
    // solc emits a CBOR map; Vyper 0.3.10+ wraps its map in a CBOR array.
    if !matches!(metadata.first(), Some(0x81..=0x9f | 0xa1..=0xbf)) {
        return false;
    }
    METADATA_KEYS.iter().any(|key| {
        let mut encoded = vec![0x60 + key.len() as u8];
        encoded.extend_from_slice(key.as_bytes());
        metadata
            .windows(encoded.len())
            .any(|window| window == encoded)
    })
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
fn wait_with_usage(child: std::process::Child) -> Result<(std::process::ExitStatus, Usage)> {
    use libc::{rusage, wait4};
    use std::os::unix::process::ExitStatusExt;

    let pid = child.id() as libc::pid_t;
    let mut status = 0;
    let mut raw = std::mem::MaybeUninit::<rusage>::zeroed();
    while unsafe { wait4(pid, &mut status, 0, raw.as_mut_ptr()) } < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error).context("waiting for child");
        }
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

    #[cfg(unix)]
    #[test]
    fn reports_child_status_when_child_ignores_stdin() {
        let input = vec![0u8; 1 << 20];
        let measured = super::run_measured(
            std::process::Command::new("sh").args(["-c", "echo early; exit 3"]),
            Some(&input),
        )
        .unwrap();
        assert_eq!(measured.output.status.code(), Some(3));
        assert_eq!(measured.output.stdout, b"early\n");
    }

    #[test]
    fn strips_known_metadata_trailer() {
        assert_eq!(stripped_cbor_len("0x6000a164736f6c630006").unwrap(), 2);
    }

    #[test]
    fn strips_vyper_metadata_trailers() {
        // Vyper 0.3.7 appends {"vyper": [0, 3, 7]}; the length excludes itself.
        assert_eq!(
            stripped_cbor_len("0x6000a165767970657283000307000b").unwrap(),
            2
        );
        // Vyper 0.4.0 appends [runtime, data, immutables, {"vyper": [0, 4, 0]}];
        // the length includes itself.
        assert_eq!(
            stripped_cbor_len("0x600084185b810400a1657679706572830004000013").unwrap(),
            2
        );
    }

    #[test]
    fn keeps_plain_bytecode_with_trailing_length_like_bytes() {
        assert_eq!(stripped_cbor_len("0x6000aabb0002").unwrap(), 6);
    }
}
