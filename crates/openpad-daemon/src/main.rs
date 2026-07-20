use openpad_core::adapter::{parse_adapter, Adapter};
use openpad_core::led::Rgb;
use openpad_daemon::config;
use openpad_daemon::doctor;
use openpad_daemon::hooks::{install_claude_hooks, uninstall_claude_hooks};
use openpad_daemon::ingest::{spawn_ingest, IngestEvent};
use openpad_daemon::input::{spawn_listener, PhysKey};
use openpad_daemon::runloop::Engine;
use openpad_dispatch::MacDispatcher;
use openpad_hid::{HidPad, PadLink, VID, PID};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("run") => run(),
        Some("listen") => listen(),
        Some("hooks") => hooks(args.next().as_deref()),
        Some("doctor") => doctor_cmd(),
        _ => usage(),
    }
}

fn usage() {
    println!("usage: openpad <command>");
    println!();
    println!("commands:");
    println!("  run              start the daemon: pad input, agent ingest, RGB status");
    println!("  listen           debug: print PhysKey events from the pad as they arrive");
    println!("  hooks install    install openpad's Claude Code hooks into ~/.claude/settings.json");
    println!("  hooks uninstall  remove openpad's hooks from ~/.claude/settings.json");
    println!("  doctor           check pad/tmux/hooks/port health");
}

fn listen() {
    let (tx, rx) = channel::<PhysKey>();
    spawn_listener(tx);
    println!("listening for pad input (Ctrl+C to quit)...");
    for event in rx {
        println!("{event:?}");
    }
}

fn doctor_cmd() {
    // Check if HID device is present
    let hid_present = check_hid_device();

    // Check if tmux is reachable
    let tmux_ok = check_tmux();

    // Check if ingest port is free
    let port_free = check_ingest_port();

    // Read settings.json
    let settings_json = read_settings_json();

    // Run checks
    let checks = doctor::run_checks(settings_json.as_deref(), hid_present, tmux_ok, port_free);

    // Print results
    let mut all_ok = true;
    for check in checks {
        let symbol = if check.ok { "✓" } else { "✗" };
        println!("{} {}", symbol, check.name);
        if !check.ok {
            println!("  {}", check.hint);
            all_ok = false;
        }
    }

    // Exit with status
    if !all_ok {
        std::process::exit(1);
    }
}

fn check_hid_device() -> bool {
    if let Ok(api) = hidapi::HidApi::new() {
        api.device_list()
            .any(|d| d.vendor_id() == VID && d.product_id() == PID && d.usage_page() == 0xFF60)
    } else {
        false
    }
}

fn check_tmux() -> bool {
    match std::process::Command::new("tmux")
        .arg("has-session")
        .output()
    {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

fn check_ingest_port() -> bool {
    TcpListener::bind("127.0.0.1:7676").is_ok()
}

fn read_settings_json() -> Option<String> {
    let settings_path = claude_settings_path();
    std::fs::read_to_string(&settings_path).ok()
}

// ---------------------------------------------------------------------------
// config / adapter loading
// ---------------------------------------------------------------------------

fn config_path() -> PathBuf {
    config::default_path().expect("openpad: could not determine $HOME for config path")
}

fn load_or_init_config() -> config::Config {
    let path = config_path();
    if !path.exists() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .unwrap_or_else(|e| panic!("openpad: failed to create {}: {e}", dir.display()));
        }
        std::fs::write(&path, config::default_toml())
            .unwrap_or_else(|e| panic!("openpad: failed to write {}: {e}", path.display()));
        println!("openpad: wrote default config to {}", path.display());
    }
    config::load(&path).unwrap_or_else(|e| {
        eprintln!("openpad: failed to load {}: {e}", path.display());
        std::process::exit(1);
    })
}

/// Adapter TOML shipped with openpad, embedded at compile time.
fn embedded_adapter(name: &str) -> Option<&'static str> {
    match name {
        "claude" => Some(include_str!("../../../adapters/claude.toml")),
        "codex" => Some(include_str!("../../../adapters/codex.toml")),
        "kimi" => Some(include_str!("../../../adapters/kimi.toml")),
        _ => None,
    }
}

fn load_adapters(cfg: &config::Config) -> Vec<Adapter> {
    cfg.agents
        .iter()
        .map(|a| {
            let src = embedded_adapter(&a.adapter).unwrap_or_else(|| {
                eprintln!(
                    "openpad: unknown adapter '{}' for agent '{}' (known: claude, codex, kimi)",
                    a.adapter, a.name
                );
                std::process::exit(1);
            });
            parse_adapter(&a.adapter, src).unwrap_or_else(|e| {
                eprintln!("openpad: adapter '{}' failed to parse: {e}", a.adapter);
                std::process::exit(1);
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// run
// ---------------------------------------------------------------------------

/// No-op pad used when no hardware is attached, so the rest of the daemon
/// (ingest, dispatch, keymap) still runs without a physical KB16-01.
struct NullPad;
impl PadLink for NullPad {
    fn send_frame(&mut self, _frame: &[Rgb; 16]) -> Result<(), String> {
        Ok(())
    }
}

fn run() {
    let cfg = load_or_init_config();
    let adapters = load_adapters(&cfg);
    let ingest_addr = cfg.ingest_addr.clone();

    let (key_tx, key_rx) = channel::<PhysKey>();
    spawn_listener(key_tx);

    let (ingest_tx, ingest_rx) = channel::<IngestEvent>();
    if let Err(e) = spawn_ingest(&ingest_addr, ingest_tx) {
        eprintln!("openpad: failed to start ingest server on {ingest_addr}: {e}");
        std::process::exit(1);
    }
    println!("openpad: ingest listening on {ingest_addr}");

    match HidPad::open() {
        Ok(pad) => {
            println!("openpad: pad connected");
            run_loop(cfg, adapters, MacDispatcher, pad, key_rx, ingest_rx)
        }
        Err(e) => {
            eprintln!("openpad: warning: pad not found ({e}); running without RGB output");
            run_loop(cfg, adapters, MacDispatcher, NullPad, key_rx, ingest_rx)
        }
    }
}

fn run_loop<P: PadLink>(
    cfg: config::Config,
    adapters: Vec<Adapter>,
    dispatcher: MacDispatcher,
    pad: P,
    key_rx: Receiver<PhysKey>,
    ingest_rx: Receiver<IngestEvent>,
) {
    let mut engine = Engine::new(cfg, adapters, dispatcher, pad);
    let start = Instant::now();
    let now_ms = || start.elapsed().as_millis() as u64;
    println!("openpad: running (Ctrl+C to quit)");
    loop {
        match key_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(k) => engine.on_key(k),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        while let Ok(ev) = ingest_rx.try_recv() {
            engine.on_ingest(ev, now_ms());
        }
        engine.on_tick(now_ms());
    }
}

// ---------------------------------------------------------------------------
// hooks install/uninstall
// ---------------------------------------------------------------------------

fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").expect("openpad: $HOME must be set"))
}

fn claude_settings_path() -> PathBuf {
    home_dir().join(".claude").join("settings.json")
}

fn shim_install_dir() -> PathBuf {
    home_dir().join(".local").join("share").join("openpad")
}

fn backup_path(path: &std::path::Path) -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut s = path.as_os_str().to_owned();
    s.push(format!(".{ts}.bak"));
    PathBuf::from(s)
}

fn hooks(sub: Option<&str>) {
    match sub {
        Some("install") => hooks_install(),
        Some("uninstall") => hooks_uninstall(),
        _ => {
            eprintln!("usage: openpad hooks install|uninstall");
            std::process::exit(1);
        }
    }
}

fn install_shims() -> PathBuf {
    let dir = shim_install_dir();
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|e| panic!("openpad: failed to create {}: {e}", dir.display()));
    let claude_shim = dir.join("claude-hook.sh");
    let codex_shim = dir.join("codex-notify.sh");
    std::fs::write(&claude_shim, include_str!("../../../shims/claude-hook.sh"))
        .unwrap_or_else(|e| panic!("openpad: failed to write {}: {e}", claude_shim.display()));
    std::fs::write(&codex_shim, include_str!("../../../shims/codex-notify.sh"))
        .unwrap_or_else(|e| panic!("openpad: failed to write {}: {e}", codex_shim.display()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for p in [&claude_shim, &codex_shim] {
            if let Ok(meta) = std::fs::metadata(p) {
                let mut perms = meta.permissions();
                perms.set_mode(0o755);
                let _ = std::fs::set_permissions(p, perms);
            }
        }
    }
    claude_shim
}

fn hooks_install() {
    let claude_shim = install_shims();

    let settings_path = claude_settings_path();
    let existing = std::fs::read_to_string(&settings_path).unwrap_or_else(|_| "{}".to_string());
    if settings_path.exists() {
        let backup = backup_path(&settings_path);
        std::fs::write(&backup, &existing)
            .unwrap_or_else(|e| panic!("openpad: failed to write backup {}: {e}", backup.display()));
        println!(
            "openpad: backed up {} to {}",
            settings_path.display(),
            backup.display()
        );
    }

    let updated = install_claude_hooks(&existing, claude_shim.to_str().expect("utf8 path"))
        .unwrap_or_else(|e| {
            eprintln!("openpad: failed to install hooks: {e}");
            std::process::exit(1);
        });

    if let Some(dir) = settings_path.parent() {
        std::fs::create_dir_all(dir)
            .unwrap_or_else(|e| panic!("openpad: failed to create {}: {e}", dir.display()));
    }
    std::fs::write(&settings_path, updated)
        .unwrap_or_else(|e| panic!("openpad: failed to write {}: {e}", settings_path.display()));
    println!(
        "openpad: installed Claude Code hooks into {}",
        settings_path.display()
    );

    let codex_shim = shim_install_dir().join("codex-notify.sh");
    println!();
    println!("Codex has no additive hooks API for `notify`; add this manually to ~/.codex/config.toml:");
    println!("  notify = [\"bash\", \"{}\"]", codex_shim.display());
}

fn hooks_uninstall() {
    let settings_path = claude_settings_path();
    let existing = match std::fs::read_to_string(&settings_path) {
        Ok(s) => s,
        Err(_) => {
            println!(
                "openpad: {} not found, nothing to do",
                settings_path.display()
            );
            return;
        }
    };

    let backup = backup_path(&settings_path);
    std::fs::write(&backup, &existing)
        .unwrap_or_else(|e| panic!("openpad: failed to write backup {}: {e}", backup.display()));
    println!(
        "openpad: backed up {} to {}",
        settings_path.display(),
        backup.display()
    );

    let updated = uninstall_claude_hooks(&existing).unwrap_or_else(|e| {
        eprintln!("openpad: failed to uninstall hooks: {e}");
        std::process::exit(1);
    });
    std::fs::write(&settings_path, updated)
        .unwrap_or_else(|e| panic!("openpad: failed to write {}: {e}", settings_path.display()));
    println!(
        "openpad: removed openpad hooks from {}",
        settings_path.display()
    );
}
