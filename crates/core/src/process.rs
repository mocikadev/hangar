//! Codex 进程检测：pgrep/ps/tasklist 多模式，排除自身。
/// 多模式进程检测：pgrep 精确名 + 全命令行模糊匹配；Windows 用 tasklist；
/// 排除自身 hangar，避免名字含 codex 即误报
pub fn codex_process_running() -> bool {
    #[cfg(target_os = "windows")]
    {
        if let Ok(o) = std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq codex.exe", "/NH"])
            .output()
        {
            let s = String::from_utf8_lossy(&o.stdout).to_ascii_lowercase();
            if s.contains("codex.exe") {
                return true;
            }
        }
        codex_running_via_ps()
    }
    #[cfg(not(target_os = "windows"))]
    {
        // 精确名：codex / codex-cli
        for name in ["codex", "codex-cli"] {
            if std::process::Command::new("pgrep")
                .args(["-x", name])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
            {
                return true;
            }
        }
        codex_running_via_ps()
    }
}

fn codex_running_via_ps() -> bool {
    let out = std::process::Command::new("ps")
        .args(["-eo", "comm,args"])
        .output();
    let Ok(o) = out else { return false };
    let text = String::from_utf8_lossy(&o.stdout);
    let self_pid = std::process::id().to_string();
    for line in text.lines() {
        let l = line.to_ascii_lowercase();
        // 排除表头与自身 switcher 进程
        if l.contains("hangar") || l.contains(&self_pid) {
            continue;
        }
        let comm = l.split_whitespace().next().unwrap_or("");
        if comm == "codex"
            || comm == "codex-cli"
            || comm.ends_with("/codex")
            || l.contains("/codex ")
            || l.contains(" codex ")
        {
            return true;
        }
    }
    false
}
