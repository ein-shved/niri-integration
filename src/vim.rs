use super::{
    error::Result,
    pstree::{ProcessTreeNode, build_process_tree},
};
use neovim_lib::{Neovim, NeovimApi, Session, neovim_api::Window};
use niri_ipc;
use nix::unistd;

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

    fn add_win(&mut self, win: Window) {
        self.windows.push(Win::new(win));
    }
}

pub struct Win {
    pub win: Window,
    num_colums: i64,
}

impl Win {
    pub fn new(win: Window) -> Self {
        Self { win, num_colums: 1 }
    }

    pub fn add_to_column(&mut self) {
        self.num_colums += 1;
    }

    pub fn get_columns(&self) -> i64 {
        self.num_colums
    }
}

pub struct Vim {
    nvim: Neovim,
    columns: Vec<WinColumn>,
    column_width_koeff: f64,
}

impl Vim {
    pub fn new(pid: i32) -> Result<Self> {
        let mut session = Self::try_session_from(
            &unistd::geteuid(),
            &build_process_tree(Some(pid))?.root,
        )?;
        session.start_event_loop();
        let mut nvim = Neovim::new(session);
        let columns = Self::calculate_columns(&mut nvim)?;
        Ok(Self {
            nvim,
            columns,
            column_width_koeff: 1.2,
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
    fn calculate_columns(nvim: &mut Neovim) -> Result<Vec<WinColumn>> {
        let wins = nvim.get_current_tabpage()?.list_wins(nvim)?;
        // Vector of columns. TODO(Shvedov) here should be used LinkedList, but it does not have an
        // insert by iter operation. LikedList now has cursor functionality, which is now available
        // only in nightly.
        let mut columns: Vec<WinColumn> = Vec::new();
        columns.reserve(wins.len());

        // For each window - create column record and find the place to store it in columns vector.
        for win in wins {
            let mut column = WinColumn::from_window(win, nvim)?;
            let mut place_to = Some(columns.len());
            for (i, c) in columns.iter_mut().enumerate() {
                // Current last less then new first - go next
                if c.end <= column.start {
                    continue;
                }
                // New last less then current first - place new before current
                if column.end <= c.start {
                    // Place before
                    place_to = Some(i);
                    break;
                }
                // Columns intersects.

                // First option - when one column is subcolumn of another.
                // Starts are the same - shrink to minimal size
                if c.start == column.start {
                    c.end = std::cmp::min(column.end, c.end);
                    place_to = None;
                    break;
                }
                // Ends are the same - shrink current to start of new and place new after
                if c.end == column.end {
                    c.end = std::cmp::min(c.end, column.start);
                    // Place after
                    place_to = Some(i + 1);
                    break;
                }
                // New is subcolumn of current
                if c.start < column.start && column.end > c.end {
                    c.end = column.start;
                    // Place after
                    place_to = Some(i + 1);
                    break;
                }
                // Current is subcolumn of new
                if column.start < c.start && c.end > column.end {
                    column.end = c.start;
                    // Place before
                    place_to = Some(i);
                    break;
                }

                // Bad option - no obvious columns. Ignore new column
                place_to = None;
                break;
            }

            if let Some(place_to) = place_to {
                columns.insert(place_to, column);
            }
        }
        Ok(columns)
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
        win: &niri_ipc::Window,
        soc: &mut niri_ipc::socket::Socket,
    ) -> Result<()> {
        soc.send(niri_ipc::Request::Action(
            niri_ipc::Action::SetWindowWidth {
                id: Some(win.id),
                change: niri_ipc::SizeChange::SetFixed(
                    self.get_desired_pixel_width() as i32,
                ),
            },
        ))??;
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
}
