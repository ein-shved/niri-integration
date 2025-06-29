//!
//! Basic utility types. The [Args] is core type which both handles command line
//! arguments and executes process. The main argument is
//! [command](Args::command). The command specify which operation to perform.
//!
//! Each [command's](Command) emum type implements [Parser] and [Runner] traits
//! to parse arguments from one side and to perform action from another.
//!
#![warn(missing_docs)]

use clap::Subcommand;
pub use clap::{Parser, ValueEnum};
use error::Result;
use niri_ipc::{Request, Response, socket::Socket};
use regex;
use std::ffi::OsString;
use std::fs::{File, read_link};
use std::io::BufRead;
use std::str;
use std::str::FromStr;
use std::{
    collections::HashMap, io, os::unix::process::CommandExt, path::PathBuf,
};

pub mod error;
mod kitty;
mod pstree;
mod vim;

/// Top-level arguments structure
#[derive(Parser, Debug)]
#[command(
    author = "Yury Shvedov (github:ein-shved)",
    version = "0.1",
    about = "Niri launcher",
    long_about = "Simple utility to smartly launch several tools withing niri."
)]
pub struct Launcher {
    /// The procedure to run
    #[command(subcommand)]
    command: Command,

    /// Optional path to niri socket
    #[arg(short, long, help = "Path to niri socket")]
    path: Option<PathBuf>,

    /// Optional template of kitty socket
    ///
    /// Will accept environment variables in view `${ENV}` and `{pid}` construction
    /// which will be replaced with pid of target kitty process
    #[arg(short, long, default_value = "${XDG_RUNTIME_DIR}/kitty-{pid}")]
    kitty_socket: String,

    /// Whenever to launch tool regardless to current focused window
    ///
    /// Launching tool will be run with default cwd withing default environment
    #[arg(short, long, default_value = "false")]
    fresh: bool,

    /// Optional niri window id to base window
    ///
    /// By default this uses focused window
    #[arg(short, long)]
    window: Option<u64>,

    /// Whether to daemonize process
    #[arg(short, long, default_value = "false")]
    daemonize: bool,
}

/// The list of supported commands
#[derive(Subcommand, Debug, Clone)]
#[command(about, long_about)]
pub enum Command {
    /// Check niri availability.
    ///
    /// Exits with success if niri is available and panics if niri is
    /// unavailable.
    #[command(about, long_about)]
    Test,

    /// Run new kitty instance.
    ///
    /// If current focused window have usable environment data (e.g. another kitty
    /// window) - the newly running window will inherit this environment (e.g. cwd).
    #[command(about, long_about)]
    Kitty,

    /// Print env for launching command.
    ///
    /// If current focused window have usable environment data (e.g. kitty
    /// window) - this will print environment to use with new window. Usable for development
    /// purposes.
    #[command(about, long_about)]
    Env,

    /// Vim-related commands.
    #[command(subcommand, about, long_about)]
    Vim(Vim),

    #[command(subcommand, about, long_about)]
    Switch(Direction),

    #[command(subcommand, about, long_about)]
    Move(Direction),
}

#[derive(Subcommand, Debug, Clone, Default)]
#[command(about, long_about)]
pub enum Vim {
    /// Run new vim instance.
    ///
    /// If current focused window have usable environment data (e.g. kitty
    /// window) - the newly running window will inherit this environment (e.g. cwd).
    #[command(about, long_about)]
    #[default]
    Run,
    /// Synchronise vim window size and offset with its content
    ///
    /// This designed to be called automatically by vim itself
    Sync,

    /// Shift vim window if it can not fit screen size
    Shift,
}

#[derive(Subcommand, Debug, Clone)]
#[command(about, long_about)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

pub struct NiriActionDirection {
    up: niri_ipc::Action,
    down: niri_ipc::Action,
    left: niri_ipc::Action,
    right: niri_ipc::Action,
}

#[derive(Default)]
struct LaunchingData {
    pub env: HashMap<String, String>,
    pub cwd: Option<String>,
    pub base_window: Option<niri_ipc::Window>,
}

impl Launcher {
    /// Run chosen subcommand
    pub fn run(self) -> Result<()> {
        if self.daemonize {
            use daemonize::Stdio;
            daemonize::Daemonize::new()
                .stdout(Stdio::keep())
                .stderr(Stdio::keep())
                .start()?;
        }
        let mut socket = if let Some(path) = self.path.as_ref() {
            Socket::connect_to(path)?
        } else {
            Socket::connect()?
        };
        let data = self.get_launching_data(&mut socket);
        match &self.command {
            Command::Test => Ok(()),
            Command::Kitty => Self::run_kitty(data),
            Command::Env => Self::print_env(data),
            Command::Vim(Vim::Run) => Self::run_vim(data, &mut socket),
            Command::Vim(Vim::Sync) => Self::sync_vim(data, &mut socket),
            Command::Vim(Vim::Shift) => Self::shift_vim(data, &mut socket),
            Command::Switch(direction) => {
                self.switch(data, &mut socket, &direction)
            }
            Command::Move(direction) => {
                self.move_window(data, &mut socket, &direction)
            }
        }
    }

    fn get_socket(&self, pid: i32) -> Result<kitty::KittySocket> {
        let pidre = regex::Regex::new(r"\{pid\}").unwrap();
        let envre = regex::Regex::new(r"\$\{([^\{\}\s]*)\}").unwrap();

        let path =
            envre.replace_all(&self.kitty_socket, |caps: &regex::Captures| {
                let var = std::env::var_os(&caps[1].to_string())
                    .unwrap_or(OsString::from(""));
                String::from(var.to_str().unwrap())
            });

        let path = pidre.replace_all(&path, format!("{pid}"));

        Ok(kitty::KittySocket::connect(PathBuf::from(
            path.to_string(),
        ))?)
    }

    fn get_launching_data_no_default(
        &self,
        socket: &mut Socket,
    ) -> Result<LaunchingData> {
        let window = self.get_base_window(socket).ok_or(io::Error::new(
            io::ErrorKind::NotFound,
            "No focused niri window",
        ))?;
        let window_c = window.clone();
        let class = window.app_id.ok_or(io::Error::new(
            io::ErrorKind::NotFound,
            "Focused niri window does not have class",
        ))?;
        if class == "kitty" {
            self.get_launching_data_from_kitty(window.pid)
        } else if class == "neovide" {
            self.get_launching_data_from_vim(window.pid)
        } else {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("Can not get launching data from {class}"),
            ))?
        }
        .map(|launching_data| launching_data.set_window(Some(window_c)))
    }

    fn get_launching_data(&self, socket: &mut Socket) -> LaunchingData {
        if self.fresh {
            LaunchingData::default()
        } else {
            self.get_launching_data_no_default(socket)
                .unwrap_or(LaunchingData::default())
        }
    }

    fn get_launching_data_from_kitty(
        &self,
        pid: Option<i32>,
    ) -> Result<LaunchingData> {
        let pid = pid.ok_or(io::Error::new(
            io::ErrorKind::NotFound,
            "Focused niri window does not have pid",
        ))?;
        let mut socket = self.get_socket(pid)?;
        let r = kitty::Command::Ls(kitty::Ls::default());
        let r = socket.request(r)?;
        let windows: Vec<kitty::OsWindow> = serde_json::from_value(r).unwrap();
        let window = Self::find_kitty_focused_window(windows).ok_or(
            io::Error::new(io::ErrorKind::NotFound, "No focused kitty window"),
        )?;
        Ok(LaunchingData::default()
            .maybe_cwd(window.cwd.to_str())
            .set_envs(window.env.into_iter()))
    }

    fn get_launching_data_from_vim(
        &self,
        pid: Option<i32>,
    ) -> Result<LaunchingData> {
        let pid = pid.ok_or(io::Error::new(
            io::ErrorKind::NotFound,
            "Focused niri window does not have pid",
        ))?;
        let environ = File::open(format!("/proc/{pid}/environ"))?;
        let lines = io::BufReader::new(environ).split(0x0);
        let launching_data =
            lines.fold(LaunchingData::default(), |launching_data, line| {
                if let Ok(line) = line {
                    if let Ok(line) = str::from_utf8(&line) {
                        if let Some((k, v)) = line.split_once("=") {
                            launching_data.add_env(k, v)
                        } else {
                            launching_data
                        }
                    } else {
                        launching_data
                    }
                } else {
                    launching_data
                }
            });
        let Ok(cwd) = PathBuf::from_str(&format!("/proc/{pid}/cwd"));
        let cwd = read_link(&cwd)
            .ok()
            .map(|cwd| String::from(cwd.to_str().unwrap()));
        Ok(launching_data.maybe_cwd(cwd))
    }

    fn run_kitty(data: LaunchingData) -> Result<()> {
        let mut proc = std::process::Command::new("kitty");

        data.env.into_iter().fold(&mut proc, |proc, (name, val)| {
            proc.arg("-o").arg(format!("env={name}={val}"))
        });

        data.cwd.map(|workdir| {
            proc.arg("-d").arg(format!("{}", workdir));
        });

        Err(proc.exec().into())
    }

    fn print_env(launching_data: LaunchingData) -> Result<()> {
        for (name, val) in launching_data.env {
            println!("{name}=\"{val}\"");
        }
        Ok(())
    }

    fn run_vim(data: LaunchingData, _: &mut Socket) -> Result<()> {
        let mut proc = std::process::Command::new("neovide");

        data.env
            .into_iter()
            .fold(&mut proc, |proc, (name, val)| proc.env(name, val));

        data.cwd.map(|workdir| {
            proc.current_dir(workdir);
        });

        Err(proc.exec().into())
    }

    fn get_vim(data: &LaunchingData) -> Result<vim::Vim> {
        vim::Vim::new(
            data.base_window
                .as_ref()
                .ok_or(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "No window provided",
                ))?
                .pid
                .ok_or(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Unknown pid of window",
                ))?,
        )
    }

    fn sync_vim(data: LaunchingData, soc: &mut Socket) -> Result<()> {
        let mut vim = Self::get_vim(&data)?;
        vim.test()?;
        vim.sync_width(&data.base_window.unwrap(), soc)
    }

    fn shift_vim(data: LaunchingData, soc: &mut Socket) -> Result<()> {
        let mut vim = Self::get_vim(&data)?;
        vim.test()?;
        vim.shift(&data.base_window.unwrap(), soc)
    }

    fn switch(
        &self,
        data: LaunchingData,
        soc: &mut Socket,
        direction: &Direction,
    ) -> Result<()> {
        let vim = if !self.fresh {
            Self::get_vim(&data)
        } else {
            Err(error::Error::from("Stub"))
        };
        if let Ok(mut vim) = vim {
            vim.switch(soc, direction)?;
        } else {
            Self::switch_niri(soc, direction)?;

        }
        Ok(())
    }

    pub fn switch_niri(soc: &mut Socket, direction: &Direction) -> Result<()> {
        soc.send(NiriActionDirection::new_focus().mk_request(direction))??;
        Ok(())
    }

    fn move_window(
        &self,
        data: LaunchingData,
        soc: &mut Socket,
        direction: &Direction,
    ) -> Result<()> {
        let vim = if !self.fresh {
            Self::get_vim(&data)
        } else {
            Err(error::Error::from("Stub"))
        };
        if let Ok(mut vim) = vim {
            vim.move_window(soc, direction)?;
        } else {
            Self::move_niri(soc, direction)?;
        }
        Ok(())
    }

    pub fn move_niri(soc: &mut Socket, direction: &Direction) -> Result<()> {
        soc.send(NiriActionDirection::new_move().mk_request(direction))??;
        Ok(())
    }

    fn find_kitty_focused_window(
        windows: Vec<kitty::OsWindow>,
    ) -> Option<kitty::Window> {
        for window in windows {
            if window.is_focused {
                for tab in window.tabs {
                    if tab.is_focused {
                        for window in tab.windows {
                            if window.is_focused {
                                return Some(window);
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn get_base_window(&self, socket: &mut Socket) -> Option<niri_ipc::Window> {
        if let Some(id) = self.window {
            if let Response::Windows(windows) =
                socket.send(Request::Windows).unwrap().unwrap()
            {
                let mut res = None;
                for window in windows.into_iter() {
                    if window.id == id {
                        res = Some(window);
                        break;
                    }
                }
                res
            } else {
                panic!("Unexpected response to Windows")
            }
        } else if let Response::FocusedWindow(window) =
            socket.send(Request::FocusedWindow).unwrap().unwrap()
        {
            window
        } else {
            panic!("Unexpected response to FocusedWindow")
        }
    }
}

impl LaunchingData {
    pub fn clear_cwd(mut self) -> Self {
        self.cwd = None;
        self
    }

    pub fn set_cwd<S>(mut self, cwd: S) -> Self
    where
        S: Into<String>,
    {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn maybe_cwd<S>(mut self, cwd: Option<S>) -> Self
    where
        S: Into<String>,
    {
        self.cwd = cwd.map(S::into);
        self
    }

    pub fn clear_env(mut self) -> Self {
        self.env.clear();
        self
    }

    pub fn add_env<K, V>(mut self, k: K, v: V) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.env.insert(k.into(), v.into());
        self
    }

    pub fn set_env<K, V>(self, k: K, v: V) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.clear_env().add_env(k, v)
    }

    pub fn add_envs<I, K, V>(self, it: I) -> Self
    where
        K: Into<String>,
        V: Into<String>,
        I: Iterator<Item = (K, V)>,
    {
        it.fold(self, |s, (k, v)| s.add_env(k, v))
    }

    pub fn set_envs<I, K, V>(self, it: I) -> Self
    where
        K: Into<String>,
        V: Into<String>,
        I: Iterator<Item = (K, V)>,
    {
        self.clear_env().add_envs(it)
    }

    pub fn set_window(mut self, window: Option<niri_ipc::Window>) -> Self {
        self.base_window = window;
        self
    }
}

impl NiriActionDirection {
    pub fn new_focus() -> Self {
        Self {
            up: niri_ipc::Action::FocusWindowOrWorkspaceUp {},
            down: niri_ipc::Action::FocusWindowOrWorkspaceDown {},
            left: niri_ipc::Action::FocusColumnOrMonitorLeft {},
            right: niri_ipc::Action::FocusColumnOrMonitorRight {},
        }
    }

    pub fn new_move() -> Self {
        Self {
            up: niri_ipc::Action::MoveWindowUpOrToWorkspaceUp {},
            down: niri_ipc::Action::MoveWindowDownOrToWorkspaceDown {},
            left: niri_ipc::Action::MoveColumnLeftOrToMonitorLeft {},
            right: niri_ipc::Action::MoveColumnRightOrToMonitorRight {},
        }
    }

    pub fn mk_action(self, direction: &Direction) -> niri_ipc::Action {
        match direction {
            Direction::Up => self.up,
            Direction::Down => self.down,
            Direction::Left => self.left,
            Direction::Right => self.right,
        }
    }

    pub fn mk_request(self, direction: &Direction) -> niri_ipc::Request {
        niri_ipc::Request::Action(self.mk_action(direction))
    }
}
