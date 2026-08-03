use std::{
    env, fs,
    io::ErrorKind,
    os::unix::{fs::MetadataExt, net::UnixDatagram},
    path::Path,
    process::Command,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use smithay_client_toolkit::{
    compositor::CompositorState,
    output::OutputState,
    reexports::{
        calloop::{EventLoop, Interest, Mode, PostAction, generic::Generic},
        calloop_wayland_source::WaylandSource,
    },
    registry::RegistryState,
    seat::SeatState,
    shell::wlr_layer::LayerShell,
    shm::Shm,
};
use wayland_client::{Connection, globals::registry_queue_init};
use waypie::app::{App, bind_control_socket, runtime_paths};

fn main() {
    if let Err(error) = run() {
        eprintln!("waypie: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let valid_arguments = arguments.is_empty()
        || matches!(arguments.as_slice(), [arg] if arg == "--show" || arg == "--kill" || arg == "--configure");
    if !valid_arguments {
        bail!("usage: waypie [--show|--kill|--configure]");
    }
    if arguments == ["--configure"] {
        let status = Command::new("waypie-config")
            .status()
            .context("cannot start the Python configurator (waypie-config)")?;
        if !status.success() {
            bail!("waypie-config exited with {status}");
        }
        return Ok(());
    }
    let command = if arguments == ["--kill"] {
        b"quit".as_slice()
    } else {
        b"show".as_slice()
    };
    let (socket_path, pid_path) = runtime_paths();
    if !arguments.is_empty() && send_command(&socket_path, command).is_ok() {
        return Ok(());
    }
    if arguments == ["--kill"] {
        return kill_from_pid_file(&pid_path);
    }
    if arguments.is_empty() && send_command(&socket_path, b"ping").is_ok() {
        return Ok(());
    }

    // A datagram path can survive a crash. It is safe to remove only after a
    // probe failed; bind_control_socket itself never replaces a live socket.
    remove_stale_runtime_files(&socket_path, &pid_path)?;

    let conn = Connection::connect_to_env().context("cannot connect to Wayland")?;
    let (globals, event_queue) = registry_queue_init(&conn)?;
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh)?;
    let layer_shell = LayerShell::bind(&globals, &qh)?;
    let shm = Shm::bind(&globals, &qh)?;
    let registry_state = RegistryState::new(&globals);
    let seat_state = SeatState::new(&globals, &qh);
    let output_state = OutputState::new(&globals, &qh);

    let mut event_loop: EventLoop<App> = EventLoop::try_new()?;
    let handle = event_loop.handle();
    WaylandSource::new(conn, event_queue).insert(handle.clone())?;
    let (socket, socket_path, pid_path) = bind_control_socket()?;
    handle.insert_source(
        Generic::new(socket, Interest::READ, Mode::Level),
        |_, socket, app| {
            let mut buffer = [0_u8; 64];
            loop {
                match socket.recv(&mut buffer) {
                    Ok(length) => app.handle_control(&buffer[..length]),
                    Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                    Err(error) => return Err(error),
                }
            }
            Ok(PostAction::Continue)
        },
    )?;
    let mut app = App::new(
        registry_state,
        seat_state,
        output_state,
        compositor,
        layer_shell,
        shm,
        qh,
    );
    app.prepare_layers();
    if arguments == ["--show"] {
        app.show()?;
    }
    while !app.exit {
        let timeout = app.visible().then_some(Duration::from_millis(16));
        event_loop.dispatch(timeout, &mut app)?;
        app.tick();
    }
    let _ = fs::remove_file(socket_path);
    let _ = fs::remove_file(pid_path);
    Ok(())
}

fn send_command(path: &std::path::Path, command: &[u8]) -> Result<()> {
    let socket = UnixDatagram::unbound()?;
    socket.send_to(command, path)?;
    Ok(())
}

fn remove_stale_runtime_files(socket_path: &Path, pid_path: &Path) -> Result<()> {
    if let Ok(pid) = read_pid(pid_path)
        && process_is_waypie(pid)?
    {
        bail!(
            "Waypie process {pid} exists but its control socket is unavailable; use waypie --kill"
        );
    }
    remove_if_present(socket_path)?;
    remove_if_present(pid_path)?;
    Ok(())
}

fn kill_from_pid_file(pid_path: &Path) -> Result<()> {
    let pid = read_pid(pid_path).context("no running Waypie instance")?;
    if !process_is_waypie(pid)? {
        remove_if_present(pid_path)?;
        bail!("refusing to kill stale or unrelated PID {pid}");
    }
    // SAFETY: the PID was parsed, ownership-checked, and identified through
    // /proc immediately before this call. SIGKILL is the emergency recovery
    // path used only when the daemon's control socket cannot answer.
    let result = unsafe { libc::kill(pid, libc::SIGKILL) };
    if result != 0 {
        return Err(std::io::Error::last_os_error()).context("cannot kill Waypie");
    }
    remove_if_present(pid_path)?;
    Ok(())
}

fn read_pid(path: &Path) -> Result<i32> {
    fs::read_to_string(path)?
        .trim()
        .parse::<i32>()
        .context("invalid Waypie PID file")
}

fn process_is_waypie(pid: i32) -> Result<bool> {
    if pid <= 1 {
        return Ok(false);
    }
    let process = Path::new("/proc").join(pid.to_string());
    let metadata = match fs::metadata(&process) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    // SAFETY: geteuid has no preconditions and does not mutate memory.
    if metadata.uid() != unsafe { libc::geteuid() } {
        return Ok(false);
    }
    let command = fs::read(process.join("cmdline")).unwrap_or_default();
    Ok(command
        .split(|byte| *byte == 0)
        .any(|part| String::from_utf8_lossy(part).contains("waypie")))
}

fn remove_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
