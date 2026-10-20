// GUI entry point (no console window). Explorer file associations launch this.
#![windows_subsystem = "windows"]

use std::time::{Duration, Instant};

use glint::ipc::{self, ClientError};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let startup = startup_commands(&args);
    // Paths are absolute now; don't keep the launch folder locked against rename/delete.
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
        let _ = std::env::set_current_dir(dir);
    }

    match ipc::Server::bind() {
        Some(server) => {
            // DX12 only: probing every backend (Vulkan layers, GL) adds ~150-200 ms to cold start.
            // Still overridable, e.g. WGPU_BACKEND=gl if a driver misbehaves.
            if std::env::var_os("WGPU_BACKEND").is_none() {
                // SAFETY: single-threaded at this point (the IPC thread starts inside `run`).
                unsafe { std::env::set_var("WGPU_BACKEND", "dx12") };
            }
            if let Err(e) = glint::app::run(server, startup) {
                fatal(&format!("Glint couldn't start its window:\n\n{e}"));
            }
        }
        // Another Glint window owns the pipe: hand the request over and exit immediately.
        None => {
            let cmds = if startup.is_empty() { vec![vec!["focus".to_string()]] } else { startup };
            for argv in cmds {
                let start = Instant::now();
                loop {
                    match ipc::send(&argv) {
                        Err(ClientError::NotRunning) if start.elapsed() < Duration::from_secs(3) => {
                            std::thread::sleep(Duration::from_millis(30));
                        }
                        _ => break,
                    }
                }
            }
        }
    }
}

/// `glintw a.png` → open; `glintw --compare-add x` (Explorer verb) → pair up for compare.
fn startup_commands(args: &[String]) -> Vec<Vec<String>> {
    let abs = |s: &String| glint::app::absolute(std::path::Path::new(s)).display().to_string();
    match args.first().map(String::as_str) {
        None => vec![],
        Some("--compare-add") => args[1..].iter().map(|f| vec!["compare-add".into(), abs(f)]).collect(),
        Some("--compare") if args.len() >= 3 => {
            let mut v = vec!["compare".to_string(), abs(&args[1]), abs(&args[2])];
            v.extend(args.get(3).cloned());
            vec![v]
        }
        Some(_) => {
            let mut v = vec!["open".to_string()];
            v.extend(args.iter().filter(|a| !a.starts_with("--")).map(abs));
            vec![v]
        }
    }
}

fn fatal(msg: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let w = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    unsafe { MessageBoxW(std::ptr::null_mut(), w(msg).as_ptr(), w("Glint").as_ptr(), MB_OK | MB_ICONERROR) };
}
