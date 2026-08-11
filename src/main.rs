use std::{
    env, fs, io::ErrorKind, os::unix::net::UnixDatagram, path::PathBuf, process::Command,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use smithay_client_toolkit::reexports::protocols::wp::viewporter::client::wp_viewporter::WpViewporter;
use smithay_client_toolkit::{
    activation::ActivationState,
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
use wayland_protocols_wlr::input_inhibitor::v1::client::zwlr_input_inhibit_manager_v1::ZwlrInputInhibitManagerV1;
use waypie::app::App;

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
    let activation_token = env::var("XDG_ACTIVATION_TOKEN").ok();
    if activation_token.is_some() {
        // SAFETY: this runs before the event loop and before Waypie creates any
        // threads. Commands launched by Waypie must not inherit and reuse
        // the compositor's one-shot startup token.
        unsafe { env::remove_var("XDG_ACTIVATION_TOKEN") };
    }
    let command: &[u8] = if arguments == ["--kill"] {
        b"quit"
    } else {
        b"show"
    };
    let socket_path = runtime_socket_path();
    if send_command(&socket_path, command).is_ok() {
        return Ok(());
    }
    if arguments == ["--kill"] {
        bail!("no running Waypie instance");
    }
    // A datagram path can survive a crash. It is safe to remove only after the
    // requested command failed; bind_control_socket never replaces a live socket.
    remove_if_present(&socket_path)?;

    let conn = Connection::connect_to_env().context("cannot connect to Wayland")?;
    let (globals, mut event_queue) = registry_queue_init(&conn)?;
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh)?;
    let layer_shell = LayerShell::bind(&globals, &qh)?;
    let shm = Shm::bind(&globals, &qh)?;
    let registry_state = RegistryState::new(&globals);
    let seat_state = SeatState::new(&globals, &qh);
    let output_state = OutputState::new(&globals, &qh);
    let activation = ActivationState::bind(&globals, &qh).ok();
    let viewporter = globals.bind::<WpViewporter, _, _>(&qh, 1..=1, ()).ok();
    let input_inhibit_manager = globals
        .bind::<ZwlrInputInhibitManagerV1, _, _>(&qh, 1..=1, ())
        .ok();

    let mut event_loop: EventLoop<App> = EventLoop::try_new()?;
    let handle = event_loop.handle();
    let (socket, socket_path) = bind_control_socket()?;
    handle.insert_source(
        Generic::new(socket, Interest::READ, Mode::Level),
        |_, socket, app| {
            let mut buffer = [0_u8; 4096];
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
        activation,
        viewporter,
        input_inhibit_manager,
        qh,
    );
    let show_on_startup = arguments.is_empty() || arguments == ["--show"];
    if show_on_startup {
        app.begin_show(activation_token)?;
        event_queue.roundtrip(&mut app)?;
        conn.flush()?;
        app.finish_show()?;
        conn.flush()?;
    }
    WaylandSource::new(conn, event_queue).insert(handle.clone())?;
    while !app.exit {
        let timeout = app.needs_tick().then_some(Duration::from_millis(10));
        event_loop.dispatch(timeout, &mut app)?;
        app.tick_hover();
        app.flush_redraw();
    }
    let reopen = app.reopen_requested;
    let _ = fs::remove_file(socket_path);
    if reopen {
        Command::new(env::current_exe()?).arg("--show").spawn()?;
    }
    Ok(())
}

fn send_command(path: &std::path::Path, command: &[u8]) -> Result<()> {
    let socket = UnixDatagram::unbound()?;
    socket.send_to(command, path)?;
    Ok(())
}

fn runtime_socket_path() -> PathBuf {
    let runtime = env::var_os("WAYPIE_RUNTIME_DIR")
        .or_else(|| env::var_os("XDG_RUNTIME_DIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("waypie");
    let display = env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland".into());
    runtime.join(format!("control-{display}.sock"))
}

fn bind_control_socket() -> Result<(UnixDatagram, PathBuf)> {
    let path = runtime_socket_path();
    fs::create_dir_all(path.parent().expect("runtime socket has a parent"))?;
    let socket =
        UnixDatagram::bind(&path).with_context(|| format!("cannot bind {}", path.display()))?;
    socket.set_nonblocking(true)?;
    Ok((socket, path))
}

fn remove_if_present(path: &std::path::Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
