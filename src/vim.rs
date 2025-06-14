use super::{
    error::Result,
    pstree::{ProcessTreeNode, build_process_tree},
};
use neovim_lib::{Neovim, NeovimApi, Session, neovim_api::Window};
use nix::unistd;

pub struct WinColumn {
    pub start: i64,
    pub end: i64,
    pub textwidth: i64,
}

impl WinColumn {
    fn from_window(win: &Window, nvim: &mut Neovim) -> Result<Self> {
        let pos = win.get_position(nvim)?;
        let width = win.get_width(nvim)?;
        let textwidth = win
            .get_buf(nvim)
            .map(|buf| {
                buf.get_option(nvim, "textwidth")
                    .map(|val| val.as_i64().unwrap_or(80))
                    .unwrap_or(80)
            })
            .unwrap_or(80);
        Ok(Self {
            start: pos.1,
            end: pos.1 + width,
            textwidth,
        })
    }
}

pub struct Vim {
    nvim: Neovim,
    columns: Vec<WinColumn>,
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
        Ok(Self { nvim, columns })
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
            let mut column = WinColumn::from_window(&win, nvim)?;
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
                    c.textwidth = std::cmp::max(column.textwidth, c.textwidth);
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

    pub fn get_columns<'a>(&'a self) -> &'a Vec<WinColumn> {
        &self.columns
    }

    pub fn get_num_columns(&mut self) -> Result<usize> {
        Ok(self.get_columns().len())
    }

    pub fn test(&mut self) -> Result<()> {
        let nums = self.get_num_columns()?;
        println!("Num columns: {}", nums);
        Ok(())
    }
}
