use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn local_hms() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as libc::time_t)
        .unwrap_or(0);
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    let filled = unsafe { !libc::localtime_r(&seconds, tm.as_mut_ptr()).is_null() };
    if !filled {
        return "00:00:00".to_string();
    }
    let tm = unsafe { tm.assume_init() };
    format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
}

pub fn process_cpu_ticks() -> Option<u64> {
    let text = fs::read_to_string("/proc/self/stat").ok()?;
    // /proc/pid/stat puts comm in parentheses; relative to the first field after
    // the closing parenthesis, utime is index 11 and stime is index 12.
    let fields: Vec<&str> = text.rsplit_once(')')?.1.split_whitespace().collect();
    let utime = fields.get(11)?.parse::<u64>().ok()?;
    let stime = fields.get(12)?.parse::<u64>().ok()?;
    Some(utime + stime)
}

pub fn os_description() -> String {
    fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                line.strip_prefix("PRETTY_NAME=")
                    .map(|value| value.trim().trim_matches('"').to_string())
            })
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| std::env::consts::OS.to_string())
}

pub fn ticks_per_second() -> u64 {
    let value = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if value > 0 {
        value as u64
    } else {
        100
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_process_cpu_ticks() {
        assert!(process_cpu_ticks().is_some());
        assert!(ticks_per_second() > 0);
    }

    #[test]
    fn formats_local_time() {
        let text = local_hms();
        assert_eq!(text.len(), 8);
        assert_eq!(text.as_bytes()[2], b':');
    }

    #[test]
    fn describes_operating_system() {
        assert!(!os_description().is_empty());
    }
}
