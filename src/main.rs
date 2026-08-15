use std::{
    env,
    ffi::OsString,
    fs,
    io::ErrorKind,
    os::unix::net::UnixDatagram,
    path::{Path, PathBuf},
    process::Command,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CliMode {
    Show,
    Kill,
    Configure,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Cli {
    mode: CliMode,
    config_path: PathBuf,
    style_path: PathBuf,
    custom_config: bool,
    custom_style: bool,
}

fn default_config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("waypie")
}

fn parse_cli(arguments: impl IntoIterator<Item = OsString>) -> Result<Cli> {
    let defaults = default_config_dir();
    let mut mode = None;
    let mut config_path = None;
    let mut style_path = None;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        let Some(option) = argument.to_str() else {
            bail!("invalid non-UTF-8 option: {argument:?}");
        };
        let requested_mode = match option {
            "--show" => Some(CliMode::Show),
            "--kill" => Some(CliMode::Kill),
            "--configure" | "--config" => Some(CliMode::Configure),
            "-c" => {
                if config_path.is_some() {
                    bail!("-c may only be specified once");
                }
                config_path = Some(PathBuf::from(
                    arguments.next().context("-c requires a config path")?,
                ));
                None
            }
            "-s" => {
                if style_path.is_some() {
                    bail!("-s may only be specified once");
                }
                style_path = Some(PathBuf::from(
                    arguments.next().context("-s requires a style path")?,
                ));
                None
            }
            _ => bail!(
                "unknown argument {option:?}; usage: waypie [--show|--kill|--configure|--config] [-c PATH] [-s PATH]"
            ),
        };
        if let Some(requested_mode) = requested_mode {
            if let Some(previous) = mode
                && previous != requested_mode
            {
                bail!("only one of --show, --kill, and --configure may be used");
            }
            mode = Some(requested_mode);
        }
    }
    let custom_config = config_path.is_some();
    let custom_style = style_path.is_some();
    Ok(Cli {
        mode: mode.unwrap_or(CliMode::Show),
        config_path: config_path.unwrap_or_else(|| defaults.join("config")),
        style_path: style_path.unwrap_or_else(|| defaults.join("style.css")),
        custom_config,
        custom_style,
    })
}

fn run() -> Result<()> {
    let cli = parse_cli(env::args_os().skip(1))?;
    if cli.mode == CliMode::Configure {
        let mut command = Command::new("waypie-config");
        if cli.custom_config {
            command.arg("-c").arg(&cli.config_path);
        }
        if cli.custom_style {
            command.arg("-s").arg(&cli.style_path);
        }
        let status = command
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
    let command: &[u8] = if cli.mode == CliMode::Kill {
        b"quit"
    } else {
        b"show"
    };
    let socket_path = runtime_socket_path();
    if send_command(&socket_path, command).is_ok() {
        return Ok(());
    }
    if cli.mode == CliMode::Kill {
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
        cli.config_path.clone(),
        cli.style_path.clone(),
        qh,
    );
    app.begin_show(activation_token)?;
    event_queue.roundtrip(&mut app)?;
    conn.flush()?;
    app.finish_show()?;
    conn.flush()?;
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
        let mut command = Command::new(env::current_exe()?);
        command.arg("--show");
        if cli.custom_config {
            command.arg("-c").arg(&cli.config_path);
        }
        if cli.custom_style {
            command.arg("-s").arg(&cli.style_path);
        }
        command.spawn()?;
    }
    Ok(())
}

fn send_command(path: &Path, command: &[u8]) -> Result<()> {
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

fn remove_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn custom_paths_can_appear_in_any_order() {
        let first = parse_cli(args(&[
            "-s",
            "/tmp/custom.css",
            "--show",
            "-c",
            "/tmp/custom.toml",
        ]))
        .unwrap();
        let second = parse_cli(args(&[
            "-c",
            "/tmp/custom.toml",
            "-s",
            "/tmp/custom.css",
            "--show",
        ]))
        .unwrap();

        assert_eq!(first, second);
        assert_eq!(first.mode, CliMode::Show);
        assert_eq!(first.config_path, Path::new("/tmp/custom.toml"));
        assert_eq!(first.style_path, Path::new("/tmp/custom.css"));
    }

    #[test]
    fn config_alias_opens_the_configurator_with_custom_paths() {
        let cli = parse_cli(args(&["--config", "-c", "/tmp/menu", "-s", "/tmp/style"])).unwrap();

        assert_eq!(cli.mode, CliMode::Configure);
        assert!(cli.custom_config);
        assert!(cli.custom_style);
    }

    #[test]
    fn paths_do_not_change_the_default_mode() {
        let cli = parse_cli(args(&["-c", "/tmp/menu"])).unwrap();
        assert_eq!(cli.mode, CliMode::Show);
        assert_eq!(cli.config_path, Path::new("/tmp/menu"));
        assert!(!cli.custom_style);
    }

    #[test]
    fn missing_paths_and_conflicting_modes_are_rejected() {
        assert!(parse_cli(args(&["-c"])).is_err());
        assert!(parse_cli(args(&["--show", "--kill"])).is_err());
    }
}
