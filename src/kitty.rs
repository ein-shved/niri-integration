use serde::{Deserialize, Serialize};
use std::io::{BufReader, BufWriter, prelude::*};
use std::{
    collections, io,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CommandPacked {
    pub cmd: String,
    pub version: KittyVersion,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_response: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

pub type KittyVersion = Vec<i8>;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum Command {
    Action(Action),
    Ls(Ls),
    Launch(Launch),
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Action {
    pub action: Vec<String>,
    #[serde(rename = "match")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_window: Option<String>,
    #[serde(rename = "self")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_window: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Ls {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_env_vars: Option<bool>,
    #[serde(rename = "match")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_window: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_tab: Option<String>,
    #[serde(rename = "self")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_window: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Launch {
    pub args: Vec<String>,
    #[serde(rename = "match")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_tab: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub var: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_title: Option<String>,
    #[serde(rename = "type")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_type: Option<LaunchType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_focus: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copy_colors: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copy_cmdline: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copy_env: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<LaunchLocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_remote_control: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_control_password: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdin_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdin_add_formatting: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spacing: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_position: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_alpha: Option<f32>,
    #[serde(rename = "self")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_tab: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_window_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_window_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_window_class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watcher: Option<Vec<PathBuf>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bias: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum LaunchType {
    Window,
    Tab,
    OsWindow,
    Overlay,
    OverlayMain,
    Background,
    Clipboard,
    Primary,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum LaunchLocation {
    After,
    Before,
    Default,
    First,
    Hsplit,
    Last,
    Neighbor,
    Split,
    Vsplit,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OsWindow {
    pub is_active: bool,
    pub is_focused: bool,
    pub tabs: Vec<Tab>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Tab {
    pub is_active: bool,
    pub is_focused: bool,
    pub windows: Vec<Window>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Window {
    pub is_active: bool,
    pub is_focused: bool,
    pub cmdline: Vec<String>,
    pub cwd: PathBuf,
    pub env: std::collections::HashMap<String, String>,
}

impl From<Command> for CommandPacked {
    fn from(value: Command) -> Self {
        let value = serde_json::to_value(value).unwrap();
        let obj = value.as_object().unwrap();
        if obj.len() > 1 {
            panic!("Invalid serilized kitty command");
        }
        let mut res = None;
        for (k, v) in obj.iter() {
            res = Some(Self {
                cmd: k.into(),
                version: vec![0, 37, 0],
                no_response: None,
                payload: Some(v.clone()),
            });
        }
        res.unwrap()
    }
}

pub struct KittySocket {
    socket: UnixStream,
}

impl KittySocket {
    pub fn connect(socket: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self {
            socket: UnixStream::connect(socket)?,
        })
    }

    fn send_with(&mut self, cmd: Command, no_response: bool) -> io::Result<()> {
        let mut cmd = CommandPacked::from(cmd);
        cmd.no_response = Some(no_response);
        let cmd = serde_json::to_string(&cmd).unwrap();
        {
            let mut writer = BufWriter::new(&mut self.socket);
            //let mut writer = BufWriter::new(std::fs::File::create("/tmp/test.txt")?);
            writer.write_all(&vec![0x1b])?;
            writer.write_all(b"P@kitty-cmd")?;
            writer.write_all(cmd.as_bytes())?;
            writer.write_all(&vec![0x1b])?;
            writer.write_all(b"\\")?;
        }

        Ok(())
    }

    pub fn request(&mut self, cmd: Command) -> io::Result<serde_json::Value> {
        self.send_with(cmd, false)?;
        let mut reader = BufReader::new(&mut self.socket);
        let mut esc = [0; 12];
        reader.read_exact(&mut esc)?;
        if esc[0] != 0x1b {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Got invalid head escape byte from kitty",
            ));
        }

        if &esc[1..] != b"P@kitty-cmd" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Got invalid head escape sequence from kitty",
            ));
        }

        let mut data = String::new();

        let mut escaped = false;
        for byte in reader.bytes() {
            let byte = byte?;
            if escaped {
                if byte != b'\\' {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Got invalid tail escape sequence from kitty",
                    ));
                } else {
                    break;
                }
            }
            if byte == 0x1b {
                escaped = true;
            } else {
                data.push(byte.into());
            }
        }

        let mut rsp: collections::HashMap<String, serde_json::Value> =
            serde_json::from_str(&data)?;
        let ok = rsp.remove("ok");
        if let Some(ok) = ok {
            if ok != true {
                Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Got error from kitty",
                ))
            } else {
                let data = rsp.remove("data");
                if let Some(data) = data {
                    if let Some(data) = data.as_str() {
                        Ok(serde_json::from_str(&data)?)
                    } else {
                        Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Kitty returns invalid data in 'data' field",
                        ))
                    }
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Kitty returns invalid response w/o 'data' field",
                    ))
                }
            }
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Kitty returns invalid response w/o 'ok' field",
            ))
        }
    }

    pub fn send(&mut self, cmd: Command) -> io::Result<()> {
        self.send_with(cmd, true)
    }
}
