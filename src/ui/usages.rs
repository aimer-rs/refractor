//! How many places use a name, and who they are: the "27 usages | 1 implementation" after a definition, and the list that
//! opens when it is pressed, or when the name is force-clicked.
//!
//! The counts are the language server's lenses, asked for a moment after the file is opened and after its text settles, and
//! when the server has finished loading the project. What is in view is a lookup, line by line; what is asked of the server,
//! and the files that are read for the list, is not done while drawing.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::Duration;

use aimer::animation::AnimInstant;
use aimer::events::element::NamedKey;
use aimer::input::TextEditingController;
use aimer::input::input::{FieldSpot, LineLabel, TextSnapshot};
use aimer::share::{ShareRef, Shared};
use aimer::{Color, ScrollController, SpanStyle, Venus};

use self::spans::spans;
use crate::ui::components::popup::Redraw;
use crate::services::lsp::{Lens, LensLoad, Lsp, Place, Status};
use crate::services::settings::ThemeMode;
use crate::model::syntax::{SyntaxHighlighter, SyntaxPalette};

/// What a list of usages says of one of them: where it is, and the line of code it is on.
#[derive(Clone, Debug, PartialEq)]
pub struct UsageRow {
    pub place: Place,
    /// Which place of the list this is, counting the places of all the files in the order they are shown. A row that is built from
    /// the list alone, one at a time, is told which one it is by this.
    pub index: usize,
    /// The file as the list names it: its path from the folder that is open. The places of a file share it.
    pub file: ShareRef<str>,
    /// The line, counted from one.
    pub number: usize,
    /// The line of code, without the blanks around it, and cut where it is very long.
    pub text: String,
    /// Which bytes of `text` are the name that is used: none where the line is not known, and where the name is past what is kept.
    pub mark: Range<usize>,
    /// The pieces `text` is drawn in: the colors of the syntax, and the name that is used in bold. Empty unless the list was
    /// gathered with [`UsageList::gather_highlighted`]. Shared, so that drawing a row again does not copy them.
    pub pieces: ShareRef<[(String, SpanStyle)]>,
}

/// One line of the list that is shown: a place, or the note of how many more there were than are listed.
#[derive(Clone, Debug, PartialEq)]
pub enum UsageEntry {
    Place(UsageRow),
    /// How many places are not listed.
    More(usize),
}

/// The usages that are in one file, in the order of the text. What a list is gathered from, before its rows are put in one line.
struct FileRows {
    path: PathBuf,
    name: ShareRef<str>,
    rows: Vec<UsageRow>,
}

/// Moves `items` into shared storage, none cloned.
fn share<T: 'static>(items: Vec<T>) -> ShareRef<[T]> {
    Shared::<[T]>::from(items.into_boxed_slice()).into_share_ref()
}

/// What a list is of: "Struct AutoIndent (src/auto_indent.rs)" and "27 usages".
#[derive(Clone, Debug, PartialEq)]
pub struct Heading {
    /// What the name is: "Struct", "Trait", "Function", where the line it is defined on says.
    pub kind: Option<&'static str>,
    pub name: String,
    /// The file it is defined in, as the folder that is open names it.
    pub definition: String,
    /// What is counted: "usages" or "implementations".
    pub noun: &'static str,
    pub count: usize,
}

impl Heading {
    /// The kind and the name: "Struct AutoIndent", or the name alone where there is no kind to say.
    pub fn title(&self) -> String {
        match self.kind {
            Some(kind) => format!("{kind} {}", self.name),
            None => self.name.clone(),
        }
    }

    /// The name of the file it is defined in, without the folders: "auto_indent.rs".
    pub fn file(&self) -> String {
        Path::new(&self.definition)
            .file_name()
            .map_or_else(
                || self.definition.clone(),
                |name| name.to_string_lossy().into_owned(),
            )
    }

    /// How many there are, in words: "27 usages", "1 implementation".
    pub fn count_text(&self) -> String {
        let noun = if self.count == 1 {
            self.noun.trim_end_matches('s')
        } else {
            self.noun
        };
        format!("{} {noun}", self.count)
    }
}

/// A list of the places that use a name, or implement it, as it is shown: by file, with the line of each.
#[derive(Clone, Debug, PartialEq)]
pub struct UsageList {
    pub heading: Heading,
    /// The places in the order they are shown, by file, and the note of how many were left out after them where some were. The card
    /// builds only the ones that are in view, from this, and it is shared with it and not copied.
    pub entries: ShareRef<[UsageEntry]>,
    /// How many more there were than are listed.
    pub omitted: usize,
    /// What the list hangs from.
    pub spot: FieldSpot,
}

impl UsageList {
    /// The most places that are listed: a name that is used more than this has the first of them.
    pub const MOST_ROWS: usize = 300;
    /// The biggest file that is read for the line of a place. A file that is bigger is listed without it.
    const MOST_FILE_BYTES: u64 = 2 * 1024 * 1024;
    /// The most characters of a line that are kept: more than a row of the card can show, which is cut where it ends.
    const MOST_TEXT: usize = 100;

    /// The list of `places`, grouped by file in the order the files are first met, and in each by line. The lines of code are
    /// read from the files, which is blocking work: this is for when a list is asked for, and not for drawing. The rows have no
    /// pieces: see [`Self::gather_highlighted`].
    pub fn gather(
        heading: Heading,
        places: &[Place],
        root: &Path,
        spot: FieldSpot,
        column: &dyn Fn(&str, u32) -> usize,
        buffers: &dyn Fn(&Path) -> Option<TextSnapshot>,
    ) -> Self {
        Self::build(heading, places, root, spot, column, buffers, None)
    }

    /// [`Self::gather`], with the pieces each row is drawn in made, with the colors of the syntax that `highlighter` gives the line.
    /// A highlighter of its own is for this: each line replaces its parse. It is done here, before the rows are shared, because
    /// what is shared is not changed.
    pub fn gather_highlighted(
        heading: Heading,
        places: &[Place],
        root: &Path,
        spot: FieldSpot,
        column: &dyn Fn(&str, u32) -> usize,
        buffers: &dyn Fn(&Path) -> Option<TextSnapshot>,
        highlighter: &SyntaxHighlighter,
    ) -> Self {
        Self::build(
            heading,
            places,
            root,
            spot,
            column,
            buffers,
            Some(highlighter),
        )
    }

    fn build(
        heading: Heading,
        places: &[Place],
        root: &Path,
        spot: FieldSpot,
        column: &dyn Fn(&str, u32) -> usize,
        buffers: &dyn Fn(&Path) -> Option<TextSnapshot>,
        highlighter: Option<&SyntaxHighlighter>,
    ) -> Self {
        let omitted = places.len().saturating_sub(Self::MOST_ROWS);
        let mut files: Vec<FileRows> = Vec::new();
        let mut at: HashMap<&Path, usize> = HashMap::new();
        for place in places.iter().take(Self::MOST_ROWS) {
            let slot = *at
                .entry(place.path.as_path())
                .or_insert_with(|| {
                    files.push(FileRows {
                        path: place.path.clone(),
                        name: ShareRef::from(
                            place
                                .path
                                .strip_prefix(root)
                                .unwrap_or(&place.path)
                                .display()
                                .to_string(),
                        ),
                        rows: Vec::new(),
                    });
                    files.len() - 1
                });
            let name = files[slot].name.clone();
            files[slot].rows.push(UsageRow {
                place: place.clone(),
                index: 0,
                file: name,
                number: place.at.line as usize + 1,
                text: String::new(),
                mark: 0..0,
                pieces: share(Vec::new()),
            });
        }
        let mut entries: Vec<UsageEntry> =
            Vec::with_capacity(places.len().min(Self::MOST_ROWS) + 1);
        for mut file in files {
            file.rows
                .sort_by_key(|row| (row.place.at.line, row.place.at.character));
            let open = buffers(&file.path);
            Self::read_lines(&mut file, column, open);
            for mut row in file.rows {
                row.index = entries.len();
                if let Some(highlighter) = highlighter {
                    Self::highlight(&mut row, highlighter);
                }
                entries.push(UsageEntry::Place(row));
            }
        }
        if omitted > 0 {
            entries.push(UsageEntry::More(omitted));
        }
        Self {
            heading,
            entries: share(entries),
            omitted,
            spot,
        }
    }

    /// Puts the line of code into each row of `file`, from the file, and marks the name in it. `column` says which byte of a line
    /// the server's character is. A file that cannot be read as text, or is too big, leaves them empty: a place is still listed.
    fn read_lines(
        file: &mut FileRows,
        column: &dyn Fn(&str, u32) -> usize,
        open: Option<TextSnapshot>,
    ) {
        // A file that is open is read as it is open: the lines the server counts are those of what it was told, which is the editor's text,
        // and the disk has not the changes that were not saved.
        if let Some(open) = open {
            for row in &mut file.rows {
                let wanted = row.place.at.line as usize;
                if wanted < open.line_count() {
                    let line = open.slice(open.line_range(wanted)).to_string();
                    Self::fill(row, line.trim_end_matches(['\r', '\n']), column);
                }
            }
            return;
        }
        let readable =
            std::fs::metadata(&file.path).is_ok_and(|meta| meta.len() <= Self::MOST_FILE_BYTES);
        let Some(text) = readable
            .then(|| std::fs::read_to_string(&file.path).ok())
            .flatten()
        else {
            return;
        };
        let mut lines = text.lines().enumerate().peekable();
        for row in &mut file.rows {
            let wanted = row.place.at.line as usize;
            while lines
                .next_if(|(number, _)| *number < wanted)
                .is_some()
            {}
            if let Some((number, line)) = lines.peek()
                && *number == wanted
            {
                Self::fill(row, line, column);
            }
        }
    }

    /// Puts `line`, the whole line of the row's place, into the row: without the blanks round it and cut where it is long, with the name
    /// that is used marked in it.
    fn fill(row: &mut UsageRow, line: &str, column: &dyn Fn(&str, u32) -> usize) {
        let trimmed = line.trim();
        let shown: String = trimmed.chars().take(Self::MOST_TEXT).collect();
        // The bytes of the name in the whole line, less the blanks that were cut from its start, and as far as is kept.
        let cut = line.len() - line.trim_start().len();
        let (from, to) = (
            column(line, row.place.at.character),
            column(line, row.place.end.character),
        );
        let start = from.saturating_sub(cut).min(shown.len());
        let end = to
            .saturating_sub(cut)
            .min(shown.len())
            .max(start);
        row.mark = if shown.is_char_boundary(start) && shown.is_char_boundary(end) {
            start..end
        } else {
            0..0
        };
        row.text = shown;
    }

    /// Makes the pieces `row` is drawn in.
    fn highlight(row: &mut UsageRow, highlighter: &SyntaxHighlighter) {
        let styled = highlighter.snippet(&row.text);
        row.pieces = share(
            spans(&row.text, &styled, row.mark.clone())
                .into_iter()
                .map(|(range, style)| (row.text[range].to_owned(), style))
                .collect(),
        );
    }

    /// The place of the row `index`, counting the rows of all the files in the order they are shown.
    pub fn place_at(&self, index: usize) -> Option<&Place> {
        // `get` of the handle is the slice itself, not an element.
        match self.entries.get().get(index)? {
            UsageEntry::Place(row) => Some(&row.place),
            UsageEntry::More(_) => None,
        }
    }

    /// How many places are listed.
    pub fn len(&self) -> usize {
        self.entries.len() - usize::from(self.omitted > 0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The colors of the counts: as they are, and under the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinkColors {
    pub rest: Color,
    pub hover: Color,
}

/// What a cell needs of the pane it is for, which is given when the pane is shown.
struct Context {
    lsp: Lsp,
    path: PathBuf,
    root: PathBuf,
    controller: TextEditingController,
    colors: LinkColors,
    mode: ThemeMode,
    /// The text of the other files that are open, which has the changes that are not saved.
    buffers: Rc<dyn Fn(&Path) -> Option<TextSnapshot>>,
}

/// The lenses of a file, and the lines they are on.
struct Loaded {
    lenses: Vec<Lens>,
    /// The line of each lens that has a label, to the lens.
    lines: HashMap<usize, usize>,
    /// How many lines the text had, which says that the lines of the lenses are still right while it has as many.
    line_count: usize,
}

impl Loaded {
    fn new(lenses: Vec<Lens>, text: &TextSnapshot) -> Self {
        let mut lines = HashMap::new();
        for (index, lens) in lenses.iter().enumerate() {
            if lens.usages_caption().is_some() || lens.implementations_caption().is_some() {
                lines
                    .entry(text.byte_to_line(lens.at.min(text.len())))
                    .or_insert(index);
            }
        }
        Self {
            lenses,
            lines,
            line_count: text.line_count(),
        }
    }
}

/// What shows a file's usages: the labels after the definitions, and the list that is open.
///
/// There is one for each pane, and it lives as long as the pane does.
#[derive(Default)]
pub struct Usages {
    context: RefCell<Option<Context>>,
    loaded: RefCell<Option<Rc<Loaded>>>,
    /// The labels that were made, for each lens, so that drawing asks for none anew.
    labels: RefCell<HashMap<usize, Rc<LineLabel>>>,
    list: RefCell<Option<Rc<UsageList>>>,
    /// Which place of the list is chosen, by the keys or the pointer: the row that is marked, and the one that Return goes to.
    selected: Cell<usize>,
    /// What the card says to bring a row into view with, which the keys call, for a row that has moved out of the part of the list that
    /// is shown. The pointer does not: the row it is on is in view.
    reveal: RefCell<Option<Rc<dyn Fn(usize, usize)>>>,
    /// When a key last moved the choice: for a moment after it, the pointer does not choose, because the list that is scrolled by the key
    /// has rows come under a pointer that has not moved.
    keyed_at: Cell<Option<AnimInstant>>,
    /// Where the list is scrolled to. It is the cell's and not the card's: a card is made again with the pane that holds it, more than
    /// one may be made, and the keys have to move the one that is on screen.
    scroll: ScrollController,
    load: RefCell<Option<LensLoad>>,
    /// Which wait for a refresh is the latest: an earlier one finds it is not and does nothing.
    generation: Cell<u64>,
    /// How many times in a row the counts were asked for again because the server could not say or was busy.
    attempts: Cell<u32>,
    /// Whether the server was idle the last time it was looked at, so that a refresh is asked for when it becomes so.
    idle: Cell<bool>,
    /// What draws the pane again, which the labels are part of, when they change.
    labels_changed: Redraw,
    /// What draws the card of the list again, when it opens or shuts.
    list_changed: Redraw,
    /// What draws one row again, by the number of the row, when it is chosen or is not any more: the row that was made last for a
    /// number is the one that is told. A choice changes two rows of a list that may have hundreds, and draws only those, and not the
    /// card, which would make every row that is in view again. At most a row for each place, however much is scrolled.
    row_hooks: RefCell<HashMap<usize, Rc<dyn Fn() -> bool>>>,
}

impl Usages {
    /// How long after the text settles the lenses are asked for again.
    pub const EDIT_DELAY: Duration = Duration::from_millis(800);
    /// How long after a pane is shown, or the server is idle, they are asked for.
    pub const SHOWN_DELAY: Duration = Duration::from_millis(150);
    /// How far after the text of a line its label stands, in logical pixels.
    pub const GAP: u32 = 18;
    /// How long after the server could not say, or was busy, the counts are asked for again, and how many times at most: the
    /// pane is not built again when the server finishes loading the project, so it looks for itself.
    pub const RETRY_DELAY: Duration = Duration::from_secs(3);
    pub const MOST_RETRIES: u32 = 120;

    /// Gives the cell the pane it is for, or tells it what changed of it. A new color makes the labels again.
    pub fn attach(
        &self,
        lsp: Lsp,
        path: PathBuf,
        root: PathBuf,
        controller: TextEditingController,
        colors: LinkColors,
        mode: ThemeMode,
        buffers: Rc<dyn Fn(&Path) -> Option<TextSnapshot>>,
    ) {
        let recolored = self
            .context
            .borrow()
            .as_ref()
            .is_some_and(|context| context.colors != colors);
        if recolored {
            self.labels.borrow_mut().clear();
        }
        *self.context.borrow_mut() = Some(Context {
            lsp,
            path,
            root,
            controller,
            colors,
            mode,
            buffers,
        });
    }

    /// Adds what is called to draw the pane again when the labels change, which says whether the pane is still there.
    pub fn on_labels_change(&self, hook: Rc<dyn Fn() -> bool>) {
        self.labels_changed.add(hook);
    }

    /// Adds what is called to draw the card of the list again when it opens or shuts, which says whether the card is still there.
    pub fn on_list_change(&self, hook: Rc<dyn Fn() -> bool>) {
        self.list_changed.add(hook);
    }

    /// Sets what is called to draw the row `index` of the list again when it is chosen, or another is, which says whether the row is
    /// still there: one that says it is not is forgotten. A row that is made again for the same number takes the place of the one
    /// before. What is called reads what is chosen when it draws, as the row that is made does.
    pub fn on_row_change(&self, index: usize, hook: Rc<dyn Fn() -> bool>) {
        self.row_hooks.borrow_mut().insert(index, hook);
    }

    /// Tells the rows `indexes` that what is chosen changed.
    fn tell_rows(&self, indexes: [usize; 2]) {
        for index in indexes {
            // Cloned out of the cell first: what draws again may ask for the row to be told again.
            let hook = self.row_hooks.borrow().get(&index).cloned();
            if let Some(hook) = hook
                && !hook()
            {
                let mut hooks = self.row_hooks.borrow_mut();
                // Not if the row was made again meanwhile, which is not gone.
                if hooks
                    .get(&index)
                    .is_some_and(|now| Rc::ptr_eq(now, &hook))
                {
                    hooks.remove(&index);
                }
            }
        }
    }

    /// Asks for the lenses again `delay` from now, unless they are asked for again before. Does nothing outside the runtime. This
    /// is for what is new: a pane shown, an edit that settled, a server that became idle; it starts the count of retries again.
    pub fn schedule(self: &Rc<Self>, delay: Duration) {
        self.attempts.set(0);
        self.schedule_load(delay);
    }

    /// Whether the counts are to be asked for again, and when: where the server could not say, and where it said while it was still
    /// loading the project, which is not all that it will count. Not for ever.
    fn retry_delay(failed: bool, idle: bool, attempts: u32) -> Option<Duration> {
        (attempts < Self::MOST_RETRIES && (failed || !idle)).then_some(Self::RETRY_DELAY)
    }

    /// Whether the server is ready and has nothing it is busy with.
    fn server_idle(&self) -> bool {
        self.context
            .borrow()
            .as_ref()
            .is_some_and(|context| {
                let summary = context.lsp.summary(&context.path);
                summary.status == Some(Status::Ready) && summary.progress.is_none()
            })
    }

    fn schedule_load(self: &Rc<Self>, delay: Duration) {
        let Some(venus) = Venus::current() else {
            return;
        };
        let wanted = self.generation.get() + 1;
        self.generation.set(wanted);
        let (this, waited) = (Rc::downgrade(self), venus.sleep(delay));
        venus.spawn(async move {
            // A runtime that has no timers answers at once.
            let _ = waited.await;
            if let Some(this) = this.upgrade()
                && this.generation.get() == wanted
            {
                this.refresh();
            }
        });
    }

    /// Looks at the server, from where the pane is built: when it has become idle, the lenses are asked for.
    pub fn watch_server(self: &Rc<Self>) {
        let idle = self.server_idle();
        if idle && !self.idle.replace(true) {
            self.schedule(Self::SHOWN_DELAY);
        } else if !idle {
            self.idle.set(false);
        }
    }

    /// Asks the server for the lenses of the file now. What it said is kept, and the pane is drawn again; where it could not say,
    /// what was kept stays.
    fn refresh(self: &Rc<Self>) {
        let Some((lsp, path, controller)) = self.context.borrow().as_ref().map(|context| {
            (
                context.lsp.clone(),
                context.path.clone(),
                context.controller.clone(),
            )
        }) else {
            return;
        };
        let old = self.load.borrow_mut().take();
        if let Some(old) = old {
            old.cancel();
        }
        // The lines are those of the text the server was asked about, and not of what it is by the time it answers.
        let text = controller.snapshot();
        let this = Rc::downgrade(self);
        let load = lsp.lenses(&path, move |lenses| {
            let Some(this) = this.upgrade() else {
                return;
            };
            this.load.borrow_mut().take();
            let failed = lenses.is_none();
            if let Some(lenses) = lenses {
                this.labels.borrow_mut().clear();
                *this.loaded.borrow_mut() = Some(Rc::new(Loaded::new(lenses, &text)));
                this.labels_changed.fire();
            }
            let attempts = this.attempts.get();
            if let Some(delay) = Self::retry_delay(failed, this.server_idle(), attempts) {
                this.attempts.set(attempts + 1);
                this.schedule_load(delay);
            }
        });
        *self.load.borrow_mut() = Some(load);
    }

    /// The label after line `line`, if its definition has one, and the lines are still those the lenses were found for.
    /// This is asked for each line in view each time the pane is drawn.
    pub fn label_of(self: &Rc<Self>, line: usize) -> Option<Rc<LineLabel>> {
        let loaded = self.loaded.borrow().clone()?;
        let index = *loaded.lines.get(&line)?;
        let context = self.context.borrow();
        let context = context.as_ref()?;
        // An edit that added or took away a line has moved them: they are asked for again, and until then there are none.
        if context.controller.value().line_count() != loaded.line_count {
            return None;
        }
        if let Some(label) = self.labels.borrow().get(&index) {
            return Some(Rc::clone(label));
        }
        let lens = &loaded.lenses[index];
        let mut label = LineLabel::new(context.colors.rest)
            .hover_color(context.colors.hover)
            .font_family(crate::ui::fonts::ui_family());
        if let Some(usages) = lens.usages_caption() {
            let this = Rc::downgrade(self);
            label = label.button(usages, move |spot| {
                Self::open(&this, index, Which::Usages, spot)
            });
        }
        if let Some(implementations) = lens.implementations_caption() {
            if lens.usages_caption().is_some() {
                label = label.text(" | ");
            }
            let this = Rc::downgrade(self);
            label = label.button(implementations, move |spot| {
                Self::open(&this, index, Which::Implementations, spot)
            });
        }
        let label = Rc::new(label);
        self.labels
            .borrow_mut()
            .insert(index, Rc::clone(&label));
        Some(label)
    }

    /// A button of a label was pressed: the list of what it counts opens beside it.
    fn open(this: &Weak<Self>, index: usize, which: Which, spot: FieldSpot) {
        let Some(this) = this.upgrade() else {
            return;
        };
        let Some(loaded) = this.loaded.borrow().clone() else {
            return;
        };
        let Some(lens) = loaded.lenses.get(index) else {
            return;
        };
        this.show(lens, which, spot);
    }

    /// Opens the list of the usages of the name that starts at the byte `at`, which a force click on its definition asks for.
    /// Whether there is one: a name that has no lens is not one this knows of, and the click is for something else to do.
    pub fn show_usages_at(&self, at: usize, spot: FieldSpot) -> bool {
        let Some(loaded) = self.loaded.borrow().clone() else {
            return false;
        };
        let Some(lens) = loaded
            .lenses
            .iter()
            .find(|lens| lens.at == at && lens.usages.is_some())
        else {
            return false;
        };
        self.show(lens, Which::Usages, spot);
        true
    }

    /// Lists who uses the name that starts at the byte `at`, hanging from `spot`: at once for a definition that has a count, which already has
    /// the places, and for any other name when the language server has said where they are. Nothing is opened for a name that is not used, or
    /// that the server cannot say.
    pub fn find_usages_at(self: &Rc<Self>, at: usize, spot: FieldSpot) {
        if self.show_usages_at(at, spot) {
            return;
        }
        let Some((lsp, path)) = self
            .context
            .borrow()
            .as_ref()
            .map(|context| (context.lsp.clone(), context.path.clone()))
        else {
            return;
        };
        let this = Rc::downgrade(self);
        lsp.references(&path, at, move |places| {
            if let (Some(this), Some(places)) = (this.upgrade(), places) {
                this.show_places(&places, "usages", at, spot);
            }
        });
    }

    fn show(&self, lens: &Lens, which: Which, spot: FieldSpot) {
        let (places, noun) = match which {
            Which::Usages => (lens.usages.as_deref().unwrap_or_default(), "usages"),
            Which::Implementations => (
                lens.implementations
                    .as_deref()
                    .unwrap_or_default(),
                "implementations",
            ),
        };
        self.show_places(places, noun, lens.at, spot);
    }

    /// Opens the list of `places`, which are the `noun` of the name that starts at the byte `at`, hanging from `spot`. Nothing is opened for no places.
    fn show_places(&self, places: &[Place], noun: &'static str, at: usize, spot: FieldSpot) {
        let Some((root, path, lsp, snapshot, mode, buffers)) =
            self.context.borrow().as_ref().map(|context| {
                (
                    context.root.clone(),
                    context.path.clone(),
                    context.lsp.clone(),
                    context.controller.snapshot(),
                    context.mode,
                    Rc::clone(&context.buffers),
                )
            })
        else {
            return;
        };
        if places.is_empty() {
            return;
        }
        let heading = Heading {
            kind: Self::kind_of(&Self::line_before(&snapshot, at)),
            name: Self::name_at(&snapshot, at),
            definition: path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .display()
                .to_string(),
            noun,
            count: places.len(),
        };
        // The server counts a character of a line as it said it would, which is not always in bytes.
        let column = |line: &str, character: u32| {
            lsp.byte_of(
                &TextSnapshot::from_text(line),
                crate::services::lsp::Position { line: 0, character },
            )
        };
        // The file of this pane is the one the pane has, and the others that are open are the workspace's.
        let (own, own_text) = (path.clone(), snapshot.clone());
        let open = move |file: &Path| {
            if file == own {
                Some(own_text.clone())
            } else {
                buffers(file)
            }
        };
        // The lines of code are colored once, when the list opens, and not each time the card is drawn for another row.
        let highlighter = SyntaxHighlighter::rust(SyntaxPalette::for_mode(mode));
        let list = UsageList::gather_highlighted(
            heading,
            places,
            &root,
            spot,
            &column,
            &open,
            &highlighter,
        );
        self.selected.set(0);
        *self.list.borrow_mut() = Some(Rc::new(list));
        self.list_changed.fire();
    }

    /// The line of `text` that the byte `at` is on, up to it.
    fn line_before(text: &TextSnapshot, at: usize) -> String {
        let at = at.min(text.len());
        let start = text.line_range(text.byte_to_line(at)).start;
        text.slice(start..at).to_string()
    }

    /// What a definition is, from the line before its name: the keyword that is last, if it is one that defines a name that is
    /// counted. `pub struct ` is a "Struct".
    fn kind_of(before: &str) -> Option<&'static str> {
        match before.split_whitespace().next_back()? {
            "struct" => Some("Struct"),
            "enum" => Some("Enum"),
            "trait" => Some("Trait"),
            "fn" => Some("Function"),
            "type" => Some("Type alias"),
            "union" => Some("Union"),
            "const" => Some("Constant"),
            "static" => Some("Static"),
            _ => None,
        }
    }

    /// The name that starts at the byte `at` of `text`.
    fn name_at(text: &TextSnapshot, at: usize) -> String {
        let mut name = String::new();
        let mut byte = at;
        while let Some(ch) = text
            .byte_at(byte)
            .filter(u8::is_ascii)
            .map(char::from)
        {
            if !(ch.is_alphanumeric() || ch == '_') {
                break;
            }
            name.push(ch);
            byte += 1;
        }
        name
    }

    /// The list that is open, if one is.
    pub fn list(&self) -> Option<Rc<UsageList>> {
        self.list.borrow().clone()
    }

    /// Which place of the list is chosen, if a list is open.
    pub fn selected(&self) -> Option<usize> {
        self.list
            .borrow()
            .as_ref()
            .map(|_| self.selected.get())
    }

    /// The scroll of the list, for the card to scroll in. Clones share it.
    pub fn scroll(&self) -> ScrollController {
        self.scroll.clone()
    }

    /// Sets what the card is asked to bring a row into view with: given the row and how many rows there are.
    pub fn on_reveal(&self, reveal: Rc<dyn Fn(usize, usize)>) {
        *self.reveal.borrow_mut() = Some(reveal);
    }

    /// Moves the choice `by` rows, down for more, and no further than the first and the last, and brings the row into view. Whether a
    /// list is open: the key is the list's then, wherever the choice can go.
    pub fn move_selection(&self, by: isize) -> bool {
        let Some(count) = self
            .list
            .borrow()
            .as_ref()
            .map(|list| list.len())
        else {
            return false;
        };
        let target = self
            .selected
            .get()
            .saturating_add_signed(by)
            .min(count.saturating_sub(1));
        self.choose(target, true);
        true
    }

    /// Chooses the first row, or the last, and brings it into view. Whether a list is open.
    pub fn select_edge(&self, last: bool) -> bool {
        let Some(count) = self
            .list
            .borrow()
            .as_ref()
            .map(|list| list.len())
        else {
            return false;
        };
        self.choose(if last { count.saturating_sub(1) } else { 0 }, true);
        true
    }

    /// How long after a key moved the choice the pointer may not move it.
    const KEY_HOLD: Duration = Duration::from_millis(400);

    /// Chooses the row `index`, which the pointer is on, and so is in view already. A row that is not there is not chosen, and the
    /// pointer does not choose for a moment after a key did: it is the list that moved under it.
    pub fn select(&self, index: usize) {
        let held = self
            .keyed_at
            .get()
            .is_some_and(|at| at.elapsed() < Self::KEY_HOLD);
        if !held
            && self
                .list
                .borrow()
                .as_ref()
                .is_some_and(|list| index < list.len())
        {
            self.choose(index, false);
        }
    }

    fn choose(&self, index: usize, by_key: bool) {
        if by_key {
            self.keyed_at.set(Some(AnimInstant::now()));
        }
        let before = self.selected.replace(index);
        if before == index {
            return;
        }
        if by_key {
            let count = self
                .list
                .borrow()
                .as_ref()
                .map_or(0, |list| list.len());
            let hook = self.reveal.borrow().clone();
            if let Some(hook) = hook {
                hook(index, count);
            }
        }
        // Not the card: that makes every row in view again, for the two that changed.
        self.tell_rows([before, index]);
    }

    /// The place that is chosen, which is gone to, and the list is shut: what Return does. None where no list is open.
    pub fn open_selected(&self) -> Option<Place> {
        let place = self
            .list
            .borrow()
            .as_ref()
            .and_then(|list| list.place_at(self.selected.get()).cloned())?;
        self.close();
        Some(place)
    }

    /// What an open list does with `key`, which the field would otherwise take for the text: the arrows move the choice, Home and End go
    /// to the first row and the last, Page Up and Page Down move by a page of rows, Return goes to the place that is chosen and Escape
    /// shuts the list. No other key is the list's, nor is any key when none is open.
    pub fn take_key(&self, key: &NamedKey) -> Taken {
        // A page of rows, as many as the card shows at once, less the one that stays for the eye to find its place by.
        const PAGE: isize = 9;
        if self.list.borrow().is_none() {
            return Taken::No;
        }
        match key {
            NamedKey::ArrowDown => self.move_selection(1),
            NamedKey::ArrowUp => self.move_selection(-1),
            NamedKey::PageDown => self.move_selection(PAGE),
            NamedKey::PageUp => self.move_selection(-PAGE),
            NamedKey::Home => self.select_edge(false),
            NamedKey::End => self.select_edge(true),
            NamedKey::Enter => {
                return self
                    .open_selected()
                    .map_or(Taken::Yes, Taken::Go);
            }
            NamedKey::Escape => {
                self.close();
                true
            }
            _ => false,
        }
        .then_some(Taken::Yes)
        .unwrap_or(Taken::No)
    }

    /// Shuts the list, if one is open.
    pub fn close(&self) {
        if self.list.borrow_mut().take().is_some() {
            self.list_changed.fire();
        }
    }
}

/// What an open list did with a key.
#[derive(Clone, Debug, PartialEq)]
pub enum Taken {
    /// It is not the list's: the text takes it.
    No,
    /// It is, and was done.
    Yes,
    /// It is, and says to go to this place, which the list was shut for.
    Go(Place),
}

/// Which of the two counts of a name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Which {
    Usages,
    Implementations,
}

pub mod spans;

#[cfg(test)]
#[path = "../tests/ui/usages.rs"]
mod tests;
