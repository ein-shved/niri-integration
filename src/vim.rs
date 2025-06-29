use super::{
    Direction, Launcher,
    error::{Error, Result},
    pstree::{ProcessTreeNode, build_process_tree},
};
use neovim_lib::{Neovim, NeovimApi, Session, neovim_api::Window};
use niri_ipc;
use nix::unistd;
use std::collections::HashMap;

pub struct WinColumn {
    pub start: i64,
    pub end: i64,
    windows: Vec<Win>,
}

impl WinColumn {
    fn from_window(win: Window, nvim: &mut Neovim) -> Result<Self> {
        let pos = win.get_position(nvim)?;
        let width = win.get_width(nvim)?;
        Ok(Self {
            start: pos.1,
            end: pos.1 + width,
            windows: vec![Win::new(win)],
        })
    }

    fn primary_window(&self) -> &Win {
        &self.windows[0]
    }

    fn primary_window_mut(&mut self) -> &mut Win {
        &mut self.windows[0]
    }

    fn textwidth(&mut self, nvim: &mut Neovim) -> i64 {
        std::cmp::max(
            80,
            self.windows.iter_mut().fold(0, |fin, win| {
                // Do not account windows which are attached to more then two columns
                if win.get_columns() > 1 {
                    fin
                } else {
                    let textwidth = win
                        .win
                        .get_buf(nvim)
                        .map(|buf| {
                            buf.get_option(nvim, "textwidth")
                                .map(|val| val.as_i64().unwrap_or(80))
                                .unwrap_or(80)
                        })
                        .unwrap_or(80);
                    std::cmp::max(textwidth, fin)
                }
            }),
        )
    }

    pub fn add_win(&mut self, win: Window) {
        self.windows.push(Win::new(win));
    }

    pub fn increase_wins_columns(&mut self) {
        for win in &mut self.windows {
            win.add_to_column();
        }
    }

    pub fn add_other(&mut self, other: &mut WinColumn) {
        // Same columns - do nothing
        if self.start == other.start && self.end == other.end {
        }
        // New - inside other
        else if self.start <= other.start && self.end >= other.end {
            // Shrink current column
            self.start = other.start;
            self.end = other.end;
            // Count new column in windows
            self.increase_wins_columns();
        }
        // Current - inside other
        else if self.start >= other.start && self.end <= other.end {
            // Count current column in new windows
            other.increase_wins_columns();
        }
        // Other cases can not be handled correctly
        else {
        }

        self.windows.append(&mut other.windows);
    }
}

pub struct Win {
    pub win: Window,
    num_colums: i64,
    config: Option<HashMap<String, neovim_lib::Value>>,
}

impl Win {
    pub fn new(win: Window) -> Self {
        Self {
            win,
            num_colums: 1,
            config: None,
        }
    }

    pub fn add_to_column(&mut self) {
        self.num_colums += 1;
    }

    pub fn get_columns(&self) -> i64 {
        self.num_colums
    }

    pub fn is_floating(&mut self, nvim: &mut Neovim) -> bool {
        self.get_config(nvim)
            .get("relative")
            .cloned()
            .unwrap_or(neovim_lib::Value::Nil)
            .as_str()
            .unwrap_or("")
            != ""
    }

    fn get_config(
        &mut self,
        nvim: &mut Neovim,
    ) -> &HashMap<String, neovim_lib::Value> {
        if self.config.is_none() {
            self.config = Some(
                nvim.session
                    .call(
                        "nvim_win_get_config",
                        vec![self.win.get_value().clone()],
                    )
                    .map(|value| {
                        value
                            .as_map()
                            .cloned()
                            .unwrap_or_else(|| Default::default())
                            .into_iter()
                            .map(|(k, v)| {
                                (
                                    String::from(
                                        k.as_str().unwrap_or("__invalid"),
                                    ),
                                    v.clone(),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_else(|_| Default::default()),
            );
        }
        self.config.as_ref().unwrap()
    }
}

pub struct Vim {
    nvim: Neovim,
    columns: Vec<WinColumn>,
    column_width_koeff: f64,
    width: i64,
    height: i64,
    niri_window: niri_ipc::Window,
}

impl Vim {
    pub fn new(niri_window: niri_ipc::Window) -> Result<Self> {
        let mut session = Self::try_session_from(
            &unistd::geteuid(),
            &build_process_tree(niri_window.pid)?.root,
        )?;
        session.start_event_loop();
        let mut nvim = Neovim::new(session);
        let (columns, width, height) = Self::calculate_columns(&mut nvim)?;
        Ok(Self {
            nvim,
            columns,
            column_width_koeff: 1.2,
            width,
            height,
            niri_window,
        })
    }

    fn try_session_from(
        uid: &unistd::Uid,
        node: &ProcessTreeNode,
    ) -> Result<Session> {
        Ok(Session::new_unix_socket(format!(
            "/run/user/{}/nvim.{}.0",
            uid, node.record.pid,
        ))
        .or_else(|err| {
            node.children.iter().fold(Err(err), |res, elem| {
                res.or_else(|_| Ok(Self::try_session_from(uid, elem)?))
            })
        })?)
    }

    // This is not very stable function. It attempt to count number of columns of windows in vim.
    // In my work I always split vertically, so this should work for me. But it may not work, when
    // someone splits vim horizontally at first.
    fn calculate_columns(
        nvim: &mut Neovim,
    ) -> Result<(Vec<WinColumn>, i64, i64)> {
        let wins = nvim.get_current_tabpage()?.list_wins(nvim)?;
        // Vector of columns. TODO(Shvedov) here should be used LinkedList, but it does not have an
        // insert by iter operation. LikedList now has cursor functionality, which is now available
        // only in nightly.
        let mut columns: Vec<WinColumn> = Vec::new();
        let (mut width, mut height) = (0, 0);
        columns.reserve(wins.len());

        // For each window - create column record and find the place to store it in columns vector.
        for win in wins {
            width = std::cmp::max(
                width,
                win.get_width(nvim).unwrap_or(0)
                    + win.get_position(nvim).unwrap_or((0, 0)).1,
            );
            height = std::cmp::max(
                height,
                win.get_height(nvim).unwrap_or(0)
                    + win.get_position(nvim).unwrap_or((0, 0)).0,
            );
            let mut new_column = WinColumn::from_window(win, nvim)?;
            if new_column.primary_window_mut().is_floating(nvim) {
                continue;
            }
            let mut place_to = Some(columns.len());
            for (i, cur_column) in columns.iter_mut().enumerate() {
                // Current last less then new first - go next
                if cur_column.end <= new_column.start {
                    continue;
                }
                // New last less then current first - place new before current
                if new_column.end <= cur_column.start {
                    // Place before
                    place_to = Some(i);
                    break;
                }
                // Columns intersects.

                // First option - when one column is subcolumn of another.
                // Starts are the same - shrink current and drop new column
                if cur_column.start == new_column.start {
                    cur_column.add_other(&mut new_column);
                    place_to = None;
                    break;
                }
                // Ends are the same - shrink current to start of new and place new after
                if cur_column.end == new_column.end {
                    cur_column.end =
                        std::cmp::min(cur_column.end, new_column.start);
                    // Wins of current belongs to new too
                    cur_column.increase_wins_columns();
                    // Place after
                    place_to = Some(i + 1);
                    break;
                }
                // New is subcolumn of current
                if cur_column.start < new_column.start
                    && new_column.end > cur_column.end
                {
                    cur_column.end = new_column.start;
                    // Wins of current belongs to new too
                    cur_column.increase_wins_columns();
                    // Place after
                    place_to = Some(i + 1);
                    break;
                }
                // Current is subcolumn of new
                if new_column.start < cur_column.start
                    && cur_column.end > new_column.end
                {
                    new_column.end = cur_column.start;
                    // Wins of current belongs to new too
                    new_column.increase_wins_columns();
                    // Place before
                    place_to = Some(i);
                    break;
                }

                // Bad option - no obvious columns. Ignore new column
                place_to = None;
                break;
            }

            if let Some(place_to) = place_to {
                columns.insert(place_to, new_column);
            }
        }
        Ok((columns, width, height))
    }

    pub fn get_columns(&self) -> &Vec<WinColumn> {
        &self.columns
    }

    pub fn get_columns_mut(&mut self) -> &mut Vec<WinColumn> {
        &mut self.columns
    }

    pub fn get_num_columns(&self) -> Result<usize> {
        Ok(self.get_columns().len())
    }

    pub fn get_pixels_for_symbol(&self) -> f64 {
        // TODO(Shvedov): calculate correctly
        8.0093
    }

    pub fn set_column_width_koeff(&mut self, koef: f64) {
        self.column_width_koeff = koef;
    }

    pub fn get_column_width_koeff(&self) -> f64 {
        self.column_width_koeff
    }

    pub fn get_desired_symbol_width(&mut self) -> i64 {
        let k = self.get_column_width_koeff();
        let cols = &mut self.columns;
        let nvim = &mut self.nvim;
        cols.iter_mut()
            .fold(0.0, |summ, c| summ + (k * (c.textwidth(nvim) as f64)))
            .round() as i64
    }

    pub fn get_desired_pixel_width(&mut self) -> i64 {
        (self.get_desired_symbol_width() as f64 * self.get_pixels_for_symbol())
            .round() as i64
    }

    pub fn get_current_symbol_width(&mut self) -> i64 {
        self.columns
            .iter()
            .fold(0, |last, c| std::cmp::max(last, c.end))
    }

    pub fn get_current_pixel_width(&mut self) -> i64 {
        (self.get_current_symbol_width() as f64 * self.get_pixels_for_symbol())
            .round() as i64
    }

    pub fn sync_width(
        &mut self,
        soc: &mut niri_ipc::socket::Socket,
    ) -> Result<()> {
        soc.send(niri_ipc::Request::Action(
            niri_ipc::Action::SetWindowWidth {
                id: Some(self.niri_window.id),
                change: niri_ipc::SizeChange::SetFixed(
                    self.get_desired_pixel_width() as i32,
                ),
            },
        ))??;
        self.shift(soc)
    }

    pub fn shift(&mut self, soc: &mut niri_ipc::socket::Socket) -> Result<()> {
        let mode = get_output_mode_of_window(&self.niri_window, soc)?;
        let win = self.nvim.get_current_win()?;
        let pos = win.get_position(&mut self.nvim)?;
        let start =
            std::cmp::max(pos.1 - 1, 0) as f64 * self.get_pixels_for_symbol();
        let end = (pos.1 + win.get_width(&mut self.nvim)?) as f64
            * self.get_pixels_for_symbol();

        let offset = if (self.niri_window.view_offset + mode.width as f64) < end
        {
            Some(end - (mode.width as f64))
        } else if self.niri_window.view_offset > start {
            Some(start)
        } else {
            None
        };

        if let Some(offset) = offset {
            soc.send(niri_ipc::Request::Action(
                niri_ipc::Action::ViewOffset {
                    id: Some(self.niri_window.id),
                    offset,
                },
            ))??;
        }

        Ok(())
    }

    pub fn test(&mut self) -> Result<()> {
        let nums = self.get_num_columns()?;
        let sym_w = self.get_desired_symbol_width();
        let pix_w = self.get_desired_pixel_width();
        println!("Num columns: {}", nums);
        println!("Desired width: sym {}/ pix {}", sym_w, pix_w);
        let sym_w = self.get_current_symbol_width();
        let pix_w = self.get_current_pixel_width();
        println!("Current width: sym {}/ pix {}", sym_w, pix_w);
        Ok(())
    }

    fn get_vim_cmd_direction(
        &mut self,
        direction: &Direction,
    ) -> Result<Option<&'static str>> {
        struct Borders {
            pub top: bool,
            pub bottom: bool,
            pub left: bool,
            pub right: bool,
        }
        let win = self.nvim.get_current_win()?;
        let (row, col) = win.get_position(&mut self.nvim)?;
        let width = col + win.get_width(&mut self.nvim)?;
        let height = row + win.get_height(&mut self.nvim)?;
        let borders = Borders {
            top: row == 0,
            bottom: height == self.height,
            left: col == 0,
            right: width == self.width,
        };

        let action = match direction {
            Direction::Up => {
                if borders.top {
                    None
                } else {
                    Some("Up")
                }
            }
            Direction::Down => {
                if borders.bottom {
                    None
                } else {
                    Some("Down")
                }
            }
            Direction::Left => {
                if borders.left {
                    None
                } else {
                    Some("Left")
                }
            }
            Direction::Right => {
                if borders.right {
                    None
                } else {
                    Some("Right")
                }
            }
        };
        Ok(action)
    }

    pub fn switch(
        &mut self,
        soc: &mut niri_ipc::socket::Socket,
        direction: &Direction,
    ) -> Result<()> {
        if let Some(action) = self.get_vim_cmd_direction(direction)? {
            self.send_window_input(&format!("<{}>", action))?;
        } else {
            Launcher::switch_niri(soc, direction)?;
        };
        Ok(())
    }

    pub fn move_window(
        &mut self,
        soc: &mut niri_ipc::socket::Socket,
        direction: &Direction,
    ) -> Result<()> {
        let rotation =
            if let Some(action) = self.get_vim_cmd_direction(direction)? {
                if action == "Up" || action == "Left" {
                    Some("R")
                } else if action == "Right" || action == "Down" {
                    Some("r")
                } else {
                    None
                }
            } else {
                None
            };
        if let Some(rotation) = rotation {
            self.send_window_input(rotation)?;
        } else {
            Launcher::move_niri(soc, direction)?;
        }
        Ok(())
    }

    pub fn close_window(
        &mut self,
        force: bool,
        soc: &mut niri_ipc::socket::Socket,
    ) -> Result<()> {
        self.nvim
            .session
            .call("nvim_win_close", vec![0.into(), force.into()])
            .map_err(
                // TODO(Shvedov): Should show the error message to vim
                |e| e.to_string(),
            )?;
        self.sync_width(soc)
    }

    fn send_window_input(&mut self, key: &str) -> Result<()> {
        let cmd = format!("<Esc><C-w>{}", key);
        self.nvim.input(&cmd)?;
        Ok(())
    }

    pub fn get_cwd(&mut self) -> Result<String> {
        Ok(self.nvim.command_output("pwd")?.as_str().into())
    }

    pub fn get_pid(&mut self) -> Result<i32> {
        self.nvim
            .call_function("getpid", Default::default())?
            .as_i64()
            .ok_or_else(|| {
                crate::error::Error::from("Can not get valid pid from vim")
            })
            .map(|v| v as i32)
    }
}

fn get_output_mode_of_window(
    win: &niri_ipc::Window,
    soc: &mut niri_ipc::socket::Socket,
) -> Result<niri_ipc::Mode> {
    let id = win
        .workspace_id
        .ok_or(String::from("Unknown workspace of window"))?;
    let reply = soc.send(niri_ipc::Request::Workspaces)??;
    let workspaces = match reply {
        niri_ipc::Response::Workspaces(workspaces) => Ok(workspaces),
        _ => Err(String::from("Unexpected response type for Workspaces")),
    }?;
    let workspace = workspaces
        .iter()
        .find(|ws| ws.id == id)
        .ok_or(String::from("Can not find workspace of window"))?;

    let outputname = workspace
        .output
        .as_ref()
        .ok_or(String::from("Window atteched to hidden workspace"))?;
    let reply = soc.send(niri_ipc::Request::Outputs)??;
    let mut outputs = match reply {
        niri_ipc::Response::Outputs(outputs) => Ok(outputs),
        _ => Err(String::from("Unexpected response type for Outputs")),
    }?;
    let output = outputs
        .get_mut(outputname)
        .ok_or(String::from("Can not find output of window"))?;

    let modeindex = output
        .current_mode
        .ok_or(String::from("Window belongs to disabled output"))?;

    if output.modes.len() <= modeindex {
        Err(Error::from(format!(
            "Output references to invalid mode: {} of {}",
            modeindex,
            output.modes.len()
        )))
    } else {
        Ok(output.modes.swap_remove(modeindex))
    }
}
