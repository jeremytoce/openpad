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
        Some("service") => service(args.next().as_deref()),
        _ => usage(),
    }
}

fn service(sub: Option<&str>) {
    use openpad_daemon::service::{log_path, plist, plist_path, LABEL};
    let home = std::env::var("HOME").unwrap_or_else(|_| {
        eprintln!("openpad: HOME not set");
        std::process::exit(1);
    });
    let ppath = plist_path(&home);
    let uid = libc_getuid();
    let domain_target = format!("gui/{uid}/{LABEL}");
    let launchctl = |args: &[&str]| {
        std::process::Command::new("launchctl")
            .args(args)
            .output()
            .map(|o| (o.status.success(), String::from_utf8_lossy(&o.stderr).trim().to_string()))
            .unwrap_or((false, "launchctl not found".into()))
    };
    match sub {
        Some("install") => {
            let binary = std::env::current_exe()
                .ok()
                .and_then(|p| p.canonicalize().ok())
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| format!("{home}/.local/bin/openpad"));
            std::fs::create_dir_all(format!("{home}/Library/LaunchAgents")).ok();
            if let Err(e) = std::fs::write(&ppath, plist(&binary, &log_path(&home))) {
                eprintln!("openpad: failed to write {ppath}: {e}");
                std::process::exit(1);
            }
            // re-bootstrap cleanly if a previous version is loaded
            launchctl(&["bootout", &format!("gui/{uid}"), &ppath]);
            let (ok, err) = launchctl(&["bootstrap", &format!("gui/{uid}"), &ppath]);
            if ok {
                println!("openpad: service installed and started ({LABEL})");
                println!("openpad: logs at {}", log_path(&home));
                println!("openpad: it now starts at login and restarts on crash.");
                println!("openpad: grant Accessibility to 'openpad' if macOS prompts.");
            } else {
                eprintln!("openpad: bootstrap failed: {err}");
                std::process::exit(1);
            }
        }
        Some("uninstall") => {
            launchctl(&["bootout", &format!("gui/{uid}"), &ppath]);
            match std::fs::remove_file(&ppath) {
                Ok(_) => println!("openpad: service uninstalled"),
                Err(e) => eprintln!("openpad: could not remove {ppath}: {e}"),
            }
        }
        Some("start") => {
            let (ok, err) = launchctl(&["bootstrap", &format!("gui/{uid}"), &ppath]);
            if ok {
                println!("openpad: service started");
            } else {
                eprintln!("openpad: start failed ({err}); is the service installed?");
                std::process::exit(1);
            }
        }
        Some("stop") => {
            // bootout (not `stop`): KeepAlive would revive a merely-stopped job.
            let (ok, err) = launchctl(&["bootout", &format!("gui/{uid}"), &ppath]);
            if ok {
                println!("openpad: service stopped (pad released; safe to use VIA)");
                println!("openpad: restart with `openpad service start`");
            } else {
                eprintln!("openpad: stop failed: {err}");
                std::process::exit(1);
            }
        }
        Some("status") => {
            let (ok, _) = launchctl(&["print", &domain_target]);
            println!(
                "openpad: service {}",
                if ok { "running" } else { "not running" }
            );
        }
        _ => {
            println!("usage: openpad service <install|uninstall|start|stop|status>");
        }
    }
}

fn libc_getuid() -> u32 {
    // std has no getuid; shelling to `id -u` avoids a libc dependency.
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(501)
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
    println!("  service install  run the daemon as a login service (launchd), no terminal needed");
    println!("  service stop     release the pad (required before editing the layout in VIA)");
    println!("  service start    restart the service after a stop");
    println!("  service status   is the service running?");
    println!("  service uninstall  remove the login service");
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

    // Check if ingest port is free, honoring the configured ingest_addr
    // (falls back to the default 127.0.0.1:7676 when no config file exists).
    let cfg = load_config_soft();
    let port_free = check_ingest_port(&cfg.ingest_addr);

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

/// Tri-state port-health check for `doctor`'s ingest-port row, against
/// whatever address the config (or the default) says openpad listens on:
///   1. Bind succeeds -> the port is free (no daemon running) -> healthy.
///   2. Bind fails, but a probe of the holder looks like our own ingest
///      server (see `doctor::port_holder_is_openpad`) -> the openpad daemon
///      itself owns the port, which is the healthy steady-state -> healthy.
///   3. Bind fails and the holder doesn't answer like openpad -> some other
///      process owns the address -> unhealthy.
/// Without step 2, running `openpad doctor` while the daemon is up (the
/// common case!) would always fail the ingest-port check, a false negative.
fn check_ingest_port(addr: &str) -> bool {
    if TcpListener::bind(addr).is_ok() {
        true
    } else {
        doctor::port_holder_is_openpad(addr)
    }
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

/// Default `ingest_addr`, matching `config::default_toml()`. Used to decide
/// whether `hooks install`/`doctor` need to plumb a non-default address
/// through to the installed hook command / port checks.
const DEFAULT_INGEST_ADDR: &str = "127.0.0.1:7676";

/// Read-only config load for commands (`hooks install`, `doctor`) that only
/// need `ingest_addr` and must never have the side effect of writing a
/// config file to disk (unlike `load_or_init_config`, used by `run`).
/// Falls back to the embedded default config when no config file exists yet,
/// or when the existing one fails to parse.
fn load_config_soft() -> config::Config {
    let path = config_path();
    if path.exists() {
        config::load(&path).unwrap_or_else(|e| {
            eprintln!(
                "openpad: warning: failed to load {} ({e}); using default ingest address",
                path.display()
            );
            config::parse(config::default_toml()).expect("embedded default config must parse")
        })
    } else {
        config::parse(config::default_toml()).expect("embedded default config must parse")
    }
}

/// The `OPENPAD_INGEST=... ` prefix to prepend to an installed hook command
/// when the configured ingest address differs from the default, so shims
/// (which default to `127.0.0.1:7676`) reach the right server. Empty string
/// when the address is the default, so the installed command is unchanged.
fn ingest_env_prefix(addr: &str) -> String {
    if addr == DEFAULT_INGEST_ADDR {
        String::new()
    } else {
        format!("OPENPAD_INGEST=http://{addr} ")
    }
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

/// Hook events wired into the Codex `[hooks]` snippet printed by
/// `hooks_install`. Mirrors the events our Claude shim already forwards,
/// restricted to the ones `adapters/codex.toml` maps to a state (plus the
/// lifecycle events Claude's own install list covers).
const CODEX_HOOK_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "SubagentStop",
    "Stop",
];

fn hooks_install() {
    let claude_shim = install_shims();
    let cfg = load_config_soft();
    let env_prefix = ingest_env_prefix(&cfg.ingest_addr);

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

    let updated = install_claude_hooks(
        &existing,
        claude_shim.to_str().expect("utf8 path"),
        &env_prefix,
    )
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

    // Codex has no additive hooks API openpad can write to automatically, so
    // we print two copy-pasteable options for ~/.codex/config.toml instead
    // of editing it. We deliberately do NOT edit ~/.codex automatically.
    let codex_shim = shim_install_dir().join("codex-notify.sh");
    let claude_shim_str = claude_shim.display();
    println!();
    println!("Codex fallback (notify, done-only): fires only on turn completion");
    println!("(DONE/ERROR); no WAITING signal while a permission prompt is open.");
    println!("Add this to ~/.codex/config.toml:");
    println!("  notify = [\"bash\", \"{}\"]", codex_shim.display());
    println!();
    println!("Codex full fidelity (hooks, recommended): Codex's own hooks system");
    println!("(stdin JSON with hook_event_name, via [hooks] in ~/.codex/config.toml or");
    println!("hooks.json) maps PermissionRequest/PreToolUse/Stop etc. to accurate");
    println!("WAITING/RUNNING/DONE states, the same as Claude. Paste this into");
    println!("~/.codex/config.toml for full fidelity (we deliberately do NOT edit");
    println!("~/.codex automatically):");
    println!();
    println!("  [hooks]");
    for ev in CODEX_HOOK_EVENTS {
        println!(
            "  {ev:<17} = \"{env_prefix}OPENPAD_AGENT=codex bash \\\"{claude_shim_str}\\\"\""
        );
    }
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
