//! `L` on a directory: the tree of sizes beneath it, in an overlay pager.
//!
//! A worker walks the directory ([`crate::fs::disk_tree::scan`]) into a tree
//! it shares with [`DiskTreeStream`], waking the loop as it goes, and each
//! drain lays the tree out again, so the sizes fill in while the walk runs. The
//! stream outlives the walk to answer the fold keys: `zr` / `zm` open or
//! close a level, `zR` / `zM` all of them, and `za` the directory at the top
//! of the view. A relayout keeps the row at the top of the view there.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError, mpsc};

use super::App;
use super::pager_stream::{DrainOutcome, PagerStream, PagerStreamCmd, PagerStreamMount, RenderCtx};
use crate::fs::disk_tree::{DiskTree, Folds, Row, RowKind, Usage};
use crate::ui::pager::PagerView;

/// Levels open when the tree first shows.
const FIRST_DEPTH: usize = 2;

/// The header line sits above the first row.
const HEADER_LINES: usize = 1;

/// The tree in a pager, laid out again as the walk adds to it or a fold key
/// changes what is open.
pub struct DiskTreeStream {
    id: u32,
    rx: mpsc::Receiver<()>,
    tree: Arc<Mutex<DiskTree>>,
    folds: Folds,
    rows: Vec<Row>,
    label: String,
    done: bool,
}

/// What sits at the top of the view, to find again after a relayout.
struct Anchor {
    parent: Option<usize>,
    name: String,
    dir: Option<usize>,
}

impl DiskTreeStream {
    fn anchor(&self, scroll: usize) -> Option<Anchor> {
        let row = self.rows.get(scroll.checked_sub(HEADER_LINES)?)?;
        let dir = match row.kind {
            RowKind::Dir { id, .. } => Some(id),
            _ => row.parent,
        };
        Some(Anchor {
            parent: row.parent,
            name: row.name.clone(),
            dir,
        })
    }

    /// The line of the directory `dir`, or of its nearest ancestor shown.
    fn line_of_dir(&self, tree: &DiskTree, dir: Option<usize>) -> Option<usize> {
        let mut dir = dir;
        while let Some(id) = dir {
            let found = self
                .rows
                .iter()
                .position(|r| matches!(r.kind, RowKind::Dir { id: d, .. } if d == id));
            if found.is_some() {
                return found.map(|i| i + HEADER_LINES);
            }
            dir = tree.parent_of(id);
        }
        None
    }

    /// Lay the tree out into `view`, the top row staying on top: `anchor`, or
    /// failing that its directory or the nearest ancestor shown.
    fn render_into(&mut self, view: &mut PagerView, ctx: &RenderCtx, anchor: Option<&Anchor>) {
        let tree = self.tree.lock().unwrap_or_else(PoisonError::into_inner);
        self.rows = tree.rows(&self.folds);
        view.title = crate::ui::disk_tree::title(
            &self.label,
            tree.total(),
            tree.unreadable,
            self.folds.depth,
            self.done,
        );
        view.lines = crate::ui::disk_tree::render(&self.rows, &ctx.theme);
        view.streaming = !self.done;
        if let Some(anchor) = anchor {
            let same = self
                .rows
                .iter()
                .position(|r| r.parent == anchor.parent && r.name == anchor.name)
                .map(|i| i + HEADER_LINES);
            if let Some(line) = same.or_else(|| self.line_of_dir(&tree, anchor.dir)) {
                view.scroll = line;
            }
        }
        drop(tree);
        view.clamp_scroll_auto();
    }
}

impl PagerStream for DiskTreeStream {
    fn id(&self) -> u32 {
        self.id
    }

    fn retain_after_finish(&self) -> bool {
        true
    }

    fn drain(&mut self, view: &mut PagerView, ctx: &RenderCtx) -> DrainOutcome {
        let mut grew = false;
        let mut ended = false;
        loop {
            match self.rx.try_recv() {
                Ok(()) => grew = true,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    ended = !self.done;
                    break;
                }
            }
        }
        if !grew && !ended {
            return DrainOutcome::Idle;
        }
        self.done |= ended;
        let anchor = self.anchor(view.scroll);
        self.render_into(view, ctx, anchor.as_ref());
        if ended {
            DrainOutcome::Finished
        } else {
            DrainOutcome::Changed
        }
    }

    fn on_pager_command(
        &mut self,
        cmd: PagerStreamCmd,
        view: &mut PagerView,
        ctx: &RenderCtx,
    ) -> bool {
        let deepest = self
            .tree
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .deepest();
        let mut anchor = self.anchor(view.scroll);
        match cmd {
            PagerStreamCmd::Deeper => self.folds.deeper(deepest),
            PagerStreamCmd::Shallower => self.folds.shallower(deepest),
            PagerStreamCmd::OpenAll => self.folds.open_all(),
            PagerStreamCmd::CloseAll => self.folds.close_all(),
            PagerStreamCmd::ToggleFold => {
                // The header's line flips the root.
                let dir = anchor.as_ref().map_or(Some(0), |a| a.dir);
                let Some(dir) = dir else {
                    return false;
                };
                self.folds.flip(dir);
                anchor = anchor.map(|_| Anchor {
                    parent: None,
                    name: String::new(),
                    dir: Some(dir),
                });
            }
            PagerStreamCmd::Rerender => {}
            PagerStreamCmd::ToggleLayout => return false,
        }
        self.render_into(view, ctx, anchor.as_ref());
        true
    }
}

impl App {
    /// Walk `root` off-thread and show the tree of sizes beneath it.
    pub(super) fn open_disk_tree(&mut self, root: PathBuf) {
        let label = crate::paths::display_tilde(&root);
        let tree = Arc::new(Mutex::new(DiskTree::new(root.clone(), label.clone())));
        let walked = Arc::clone(&tree);
        let title =
            crate::ui::disk_tree::title(&label, Usage::default(), 0, Some(FIRST_DEPTH), false);
        self.spawn_pager_stream(
            PagerStreamMount::Overlay {
                title,
                line_count_hint: None,
            },
            move |tx: crate::fs::WakingSender<()>| {
                crate::fs::disk_tree::scan(&root, &walked, || tx.send(()).is_ok());
            },
            move |id, rx| {
                Box::new(DiskTreeStream {
                    id,
                    rx,
                    tree,
                    folds: Folds::new(FIRST_DEPTH),
                    rows: Vec::new(),
                    label,
                    done: false,
                })
            },
        );
        if let Some(view) = self.view.project_mut().pager.as_mut() {
            view.show_line_numbers = false;
            view.wrap = false;
        }
    }
}
