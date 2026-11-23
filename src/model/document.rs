//! A file open in the editor: its text, its caret, its syntax, and whether it has changed.

use std::cell::{Cell, RefCell};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

use crate::model::completion::Source;
use aimer::FocusNode;
use aimer::input::{ControllerListener, TextEditingController, TextEditingValue};

use aimer::MarkdownLivePreview;

use crate::services::settings::ThemeMode;
use crate::model::syntax::{SyntaxHighlighter, SyntaxPalette};

/// What the file on disk is, compared with what the document last saw of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disk {
    /// As the document left it, by writing it or by reading it.
    Same,
    /// Written by something else since.
    Changed,
    /// Not there any more.
    Gone,
}

/// What reading a file again did to the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reloaded {
    /// The text was what the file says already.
    Unchanged,
    /// The text is what the file says now.
    Replaced,
}

/// How a file looked when it was last read or written: when it was changed and how long it is. Two files that look alike are taken
/// for the same one, which a program that changes a file and keeps both can fool, and which no editor can tell without reading it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
}

impl Stamp {
    fn of(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        Some(Self {
            modified: metadata.modified().ok(),
            len: metadata.len(),
        })
    }
}

/// What a file is written in, which decides how it is colored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Rust,
    /// TOML, as `Cargo.toml` is written.
    Toml,
    /// Markdown, which is shown rendered wherever the caret is not.
    Markdown,
    PlainText,
}

impl Language {
    /// The language of a file called `path`, by its extension.
    pub fn of_path(path: &Path) -> Self {
        match path
            .extension()
            .and_then(|extension| extension.to_str())
        {
            Some(extension) if extension.eq_ignore_ascii_case("rs") => Self::Rust,
            Some(extension) if extension.eq_ignore_ascii_case("toml") => Self::Toml,
            Some(extension)
                if extension.eq_ignore_ascii_case("md")
                    || extension.eq_ignore_ascii_case("markdown") =>
            {
                Self::Markdown
            }
            _ => Self::PlainText,
        }
    }

    /// Whether the language has items to complete a word with.
    pub fn has_completions(self) -> bool {
        matches!(self, Self::Rust | Self::Toml)
    }

    /// What begins a line comment in the language, where it has a line comment that the commenting shortcut can make.
    pub fn line_comment(self) -> Option<&'static str> {
        match self {
            Self::Rust => Some(crate::model::line_comment::LineComment::RUST),
            Self::Toml => Some(crate::model::line_comment::LineComment::TOML),
            Self::Markdown | Self::PlainText => None,
        }
    }

    /// The name shown in the status bar.
    pub fn label(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::Toml => "TOML",
            Self::Markdown => "Markdown",
            Self::PlainText => "Plain Text",
        }
    }
}

impl Document {
    /// The name of the manifest of a crate, which the Cargo schema is for.
    pub const CARGO_MANIFEST: &'static str = "Cargo.toml";

    /// Whether this is the `Cargo.toml` of a crate: other TOML files are not the Cargo schema's to check.
    pub fn is_cargo_manifest(&self) -> bool {
        self.language == Language::Toml && self.name == Self::CARGO_MANIFEST
    }
}

/// A file that is open. It owns its editing state (the text, the caret, the undo history), so
/// switching to another file and back leaves it as it was.
pub struct Document {
    /// Told apart from the other documents, open or closed.
    pub id: u64,
    pub name: String,
    pub path: PathBuf,
    pub language: Language,
    /// Whether the file's lines end in CRLF, which the status bar says. It is read from the first
    /// line break of the file when it is opened.
    pub crlf: bool,
    pub controller: TextEditingController,
    /// The text area's focus, which is the document's so that something outside the text area can ask for
    /// the caret to be where it is.
    pub focus: FocusNode,
    /// Where the items to complete a word come from, which the workspace sets for a file that a language server
    /// is run for. Without one, the editor offers the keywords of the language.
    completions: RefCell<Option<Rc<dyn Source>>>,
    /// Made once: a new highlighter on every rebuild would parse the file again. `None` for a
    /// language with no syntax colors.
    pub syntax: Option<SyntaxHighlighter>,
    /// The live preview of a markdown file, made once for the same reason: it keeps the parse. It is made
    /// again when the theme changes, since a preview is made with its colors.
    markdown: RefCell<Option<MarkdownLivePreview>>,
    /// The theme the colors of `syntax` and `markdown` are those of.
    theme: Cell<ThemeMode>,
    /// The text as it is on disk: what the document is compared with to know it has changed.
    saved: Rc<RefCell<TextEditingValue>>,
    dirty: Rc<Cell<bool>>,
    /// Called when the document becomes changed or unchanged, so what shows it can be redrawn.
    notify: Rc<dyn Fn()>,
    /// What is called when the text is changed, which is each change and not only the one that makes the document changed.
    edited: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
    /// How the file looked when it was last read or written, which is how a change that something else made to it is known. None
    /// where it was not there.
    stamp: Cell<Option<Stamp>>,
    /// Whether the text is being replaced by what the file says, in which time the document is not said to be changed.
    reloading: Rc<Cell<bool>>,
    /// Keeps `dirty` following the controller for as long as the document is open.
    _listener: ControllerListener,
}

impl PartialEq for Document {
    /// Documents are the same when they have the same identity: what a widget that watches the
    /// document shown compares.
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Document {
    /// How long may pass between two edits for them to be one undo: what is typed without a pause this long is taken
    /// back at once.
    pub const UNDO_WINDOW: std::time::Duration = std::time::Duration::from_millis(1000);

    /// Opens the file at `path`, or says why it cannot be read. A file that is not UTF-8 is not
    /// opened: the editor holds text. `notify` is called whenever the document becomes changed or
    /// unchanged, which is not on every keystroke.
    pub fn open(id: u64, path: &Path, notify: Rc<dyn Fn()>) -> io::Result<Rc<Self>> {
        let text = std::fs::read_to_string(path)?;
        let crlf = text
            .find('\n')
            .is_some_and(|at| at > 0 && text.as_bytes()[at - 1] == b'\r');
        // The text goes straight into the editor's buffer, without a copy made on the way. A file
        // opens at its first line, and a controller made with text starts at its end.
        let controller = TextEditingController::with_str(&text);
        controller.select(0, 0);
        let saved = controller.value();
        Ok(Self::from_parts(
            id, path, crlf, controller, saved, false, notify,
        ))
    }

    /// Creates a document at `new_path` carrying over this document's current text, selection, and
    /// unsaved state after its file or an enclosing folder was renamed or moved on disk.
    pub fn relocated(&self, id: u64, new_path: &Path, notify: Rc<dyn Fn()>) -> Rc<Self> {
        let current = self.controller.value();
        let selection = current.selection().range();
        let controller = TextEditingController::with_str(&current.text());
        controller.select_bytes(selection.start, selection.end);
        let saved = self.saved.borrow().clone();
        let doc = Self::from_parts(
            id,
            new_path,
            self.crlf,
            controller,
            saved,
            self.dirty.get(),
            notify,
        );
        doc.set_theme(self.theme.get());
        doc
    }

    fn from_parts(
        id: u64,
        path: &Path,
        crlf: bool,
        controller: TextEditingController,
        saved_value: TextEditingValue,
        initially_dirty: bool,
        notify: Rc<dyn Fn()>,
    ) -> Rc<Self> {
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let language = Language::of_path(path);
        controller.set_undo_merge_window(Self::UNDO_WINDOW);
        let saved = Rc::new(RefCell::new(saved_value));
        let dirty = Rc::new(Cell::new(initially_dirty));
        let edited: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::default();
        let reloading: Rc<Cell<bool>> = Rc::default();
        let listener = {
            let (controller, saved, dirty, notify, edited, reloading) = (
                controller.clone(),
                Rc::clone(&saved),
                Rc::clone(&dirty),
                Rc::clone(&notify),
                Rc::clone(&edited),
                Rc::clone(&reloading),
            );
            // What the text was the last time it was looked at, to tell an edit from a move of the caret.
            let last = RefCell::new(controller.value());
            controller.clone().listen(move |_| {
                // Moving the caret changes the value but not the text: only the text counts.
                let value = controller.value();
                let edit = !value.same_text(&last.borrow());
                let changed = !value.same_text(&saved.borrow());
                if edit {
                    *last.borrow_mut() = value;
                }
                // A reload is one step from what the document was to what the file is, and not through a text that is changed.
                if !reloading.get() && dirty.replace(changed) != changed {
                    notify();
                }
                if edit {
                    // Not while the hook is borrowed: what it does may set it again.
                    let hook = edited.borrow().clone();
                    if let Some(hook) = hook {
                        hook();
                    }
                }
            })
        };
        let syntax = match language {
            Language::Rust => Some(SyntaxHighlighter::rust(SyntaxPalette::dark())),
            Language::Toml => Some(SyntaxHighlighter::toml(SyntaxPalette::dark())),
            Language::Markdown | Language::PlainText => None,
        };
        // Every document starts in the dark theme; what shows it says the theme it is shown in with
        // [`Document::set_theme`], which is nothing to do when they are the same.
        let markdown =
            (language == Language::Markdown).then(|| crate::ui::markdown::preview(ThemeMode::Dark));
        Rc::new(Self {
            id,
            name,
            path: path.to_path_buf(),
            language,
            crlf,
            controller,
            focus: FocusNode::new(),
            completions: RefCell::new(None),
            syntax,
            markdown: RefCell::new(markdown),
            theme: Cell::new(ThemeMode::Dark),
            saved,
            dirty,
            notify,
            edited,
            stamp: Cell::new(Stamp::of(path)),
            reloading,
            _listener: listener,
        })
    }

    /// Whether the file on disk is what the document last saw of it. A save by the document is not a change.
    pub fn disk(&self) -> Disk {
        match (self.stamp.get(), Stamp::of(&self.path)) {
            (Some(_), None) => Disk::Gone,
            (None, None) => Disk::Same,
            (None, Some(_)) => Disk::Changed,
            (Some(seen), Some(now)) => {
                if seen == now {
                    Disk::Same
                } else {
                    Disk::Changed
                }
            }
        }
    }

    /// Takes the file as it is for what the document has seen of it, without reading it: a change is then not told again.
    pub fn acknowledge_disk(&self) {
        self.stamp.set(Stamp::of(&self.path));
    }

    /// Reads the file again and makes its text the document's, as one edit that can be taken back: what is in the text and not in the
    /// file goes, and the caret and the selection stay with the text they were in, or go to the start of what replaced it where that was
    /// replaced. The document is unchanged after, whatever it was before. A file that cannot be read as text is not reloaded, and the
    /// document is as it was.
    pub fn reload(&self) -> io::Result<Reloaded> {
        let text = std::fs::read_to_string(&self.path)?;
        // The state of the file as it is read, and not as it is by the time it is looked at again: what is written in between is a change.
        let stamp = Stamp::of(&self.path);
        let current = self.controller.snapshot();
        let replaced = match current.diff(&aimer::input::input::TextSnapshot::from_text(&text)) {
            None => Reloaded::Unchanged,
            Some((old, new)) => {
                let selection = self.controller.value().selection().range();
                let (anchor, focus) = (selection.start, selection.end);
                let moved = |at: usize| {
                    if at <= old.start {
                        at
                    } else if at >= old.end {
                        at - old.len() + new.len()
                    } else {
                        old.start
                    }
                };
                self.reloading.set(true);
                self.controller
                    .replace_range(old.clone(), &text[new.clone()]);
                self.controller
                    .select_bytes(moved(anchor), moved(focus));
                self.reloading.set(false);
                Reloaded::Replaced
            }
        };
        self.stamp.set(stamp);
        *self.saved.borrow_mut() = self.controller.value();
        if self.dirty.replace(false) {
            (self.notify)();
        }
        Ok(replaced)
    }

    /// Sets what is called when the text is changed, each time it is: typed, pasted, taken back or put back. Not when the caret
    /// moves, nor when the file is saved. What was set before is replaced.
    pub fn on_edit(&self, hook: Rc<dyn Fn()>) {
        *self.edited.borrow_mut() = Some(hook);
    }

    /// The live preview of a markdown file, which shares its parse with the one the document keeps.
    pub fn markdown(&self) -> Option<MarkdownLivePreview> {
        self.markdown.borrow().clone()
    }

    /// Colors the syntax and the markdown as the theme `mode` does. It does nothing when the document is
    /// in that theme already, so it may be called each time the document is shown. A markdown preview is
    /// made again, and parses again when it is next drawn; the syntax of code is only recolored.
    pub fn set_theme(&self, mode: ThemeMode) {
        if self.theme.replace(mode) == mode {
            return;
        }
        if let Some(syntax) = &self.syntax {
            syntax.set_palette(SyntaxPalette::for_mode(mode));
            syntax.set_doc_theme(crate::ui::markdown::doc_theme(mode));
        }
        if self.markdown.borrow().is_some() {
            *self.markdown.borrow_mut() = Some(crate::ui::markdown::preview(mode));
        }
    }

    /// Says where the items to complete a word are to come from.
    pub fn set_completions(&self, source: Rc<dyn Source>) {
        *self.completions.borrow_mut() = Some(source);
    }

    /// Where the items to complete a word come from, if anywhere is set.
    pub fn completions(&self) -> Option<Rc<dyn Source>> {
        self.completions.borrow().clone()
    }

    /// Puts the caret at the byte `at` of the text, and the focus in the editor: for a place that something else
    /// has pointed at, such as a problem. The text area scrolls to show it. A byte inside a character is the start
    /// of it, and one past the end is the end.
    pub fn go_to(&self, at: usize) {
        self.controller.select_bytes(at, at);
        self.focus.request_focus();
        // The text area scrolls to the caret in its next layout, and a layout is done for a frame. Most places come
        // from a click, which asks for one; one that a language server's answer points at comes from no event.
        aimer::events::window::request_animation_frame();
    }

    /// Puts the caret at the start of the bytes `range` of the text, and the focus in the editor, and asks the text area to
    /// show the range in the middle of its view with a mark on it: for a place that is far from where the eye is, such as
    /// the definition of a name. Ends are cut to the text and to whole characters.
    pub fn show(&self, range: std::ops::Range<usize>) {
        self.controller.locate(range);
        self.focus.request_focus();
        // The text area takes the request in its next layout, and a layout is done for a frame; see `go_to`.
        aimer::events::window::request_animation_frame();
    }

    /// What the status bar calls the file's line endings.
    pub fn line_ending(&self) -> &'static str {
        if self.crlf { "CRLF" } else { "LF" }
    }

    /// Whether the text differs from what is on disk.
    pub fn is_dirty(&self) -> bool {
        self.dirty.get()
    }

    /// Writes the text to the file it came from, and makes it the text the document is compared
    /// with. The file is replaced whole or not at all: a crash or a full disk in the middle leaves
    /// the old file, and not half of the new one.
    pub fn save(&self) -> io::Result<()> {
        let value = self.controller.value();
        write_atomically(&self.path, &value)?;
        self.stamp.set(Stamp::of(&self.path));
        *self.saved.borrow_mut() = value;
        if self.dirty.replace(false) {
            (self.notify)();
        }
        Ok(())
    }
}

/// Writes `value` to `path` by writing a file beside it and renaming that over it.
fn write_atomically(path: &Path, value: &TextEditingValue) -> io::Result<()> {
    // Through a link, to the file it points to: renaming over the link would replace the link.
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let name = path
        .file_name()
        .map_or_else(|| "file".into(), |name| name.to_string_lossy().into_owned());
    let temporary = path.with_file_name(format!(".{name}.refractor-save"));
    let written = (|| {
        let file = std::fs::File::create(&temporary)?;
        let mut writer = BufWriter::new(file);
        value.write_to(&mut writer)?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        // The file keeps the permissions it had.
        if let Ok(metadata) = std::fs::metadata(&path) {
            std::fs::set_permissions(&temporary, metadata.permissions())?;
        }
        std::fs::rename(&temporary, &path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tree::Scratch;

    fn counter() -> (Rc<Cell<usize>>, Rc<dyn Fn()>) {
        let count = Rc::new(Cell::new(0));
        let counted = Rc::clone(&count);
        (count, Rc::new(move || counted.set(counted.get() + 1)))
    }

    #[test]
    fn what_is_typed_in_a_burst_is_one_undo() {
        let scratch = Scratch::new("undo");
        let document =
            Document::open(5, &scratch.file("a.rs", "fn () {}\n"), counter().1).expect("opens");
        let controller = &document.controller;
        // Typed one after the other, as fast as a test types: far inside the window.
        for (at, typed) in [(3, "m"), (4, "a"), (5, "i"), (6, "n")] {
            assert!(controller.replace_range(at..at, typed));
        }
        assert_eq!(controller.value().text(), "fn main() {}\n");
        assert!(controller.undo());
        assert_eq!(
            controller.value().text(),
            "fn () {}\n",
            "all of it, and not the last character"
        );
        assert!(!controller.undo());
        assert!(
            Document::UNDO_WINDOW >= std::time::Duration::from_millis(250),
            "a window shorter than a pause between words would split a word"
        );
    }

    #[test]
    fn markdown_files_are_markdown_and_have_a_preview_and_no_syntax() {
        for name in ["README.md", "notes.MD", "doc.markdown"] {
            assert_eq!(
                Language::of_path(Path::new(name)),
                Language::Markdown,
                "{name}"
            );
        }
        let scratch = Scratch::new("markdown");
        let (_, notify) = counter();
        let document =
            Document::open(3, &scratch.file("a.md", "# Title\n"), notify).expect("opens");
        assert_eq!(document.language, Language::Markdown);
        assert_eq!(document.language.label(), "Markdown");
        assert!(document.markdown().is_some() && document.syntax.is_none());
        let rust =
            Document::open(4, &scratch.file("a.rs", "fn a() {}\n"), counter().1).expect("opens");
        assert!(rust.markdown().is_none());
    }

    #[test]
    fn a_document_takes_the_colors_of_the_theme_it_is_shown_in() {
        use crate::services::settings::ThemeMode;
        use aimer::input::input::{HighlightContext, TextSnapshot};
        let scratch = Scratch::new("theme");
        let rust_text = "fn a() {}\n";
        let rust = Document::open(1, &scratch.file("a.rs", rust_text), counter().1).expect("opens");
        let keyword = |document: &Document| {
            let syntax = document
                .syntax
                .as_ref()
                .expect("Rust has syntax");
            let text = TextSnapshot::from_text(rust_text);
            syntax
                .ranges(&text, 0..2)
                .first()
                .and_then(|run| run.style.color)
        };
        assert_eq!(
            keyword(&rust),
            Some(SyntaxPalette::dark().keyword),
            "a document starts in the dark theme"
        );
        rust.set_theme(ThemeMode::Light);
        assert_eq!(keyword(&rust), Some(SyntaxPalette::light().keyword));
        rust.set_theme(ThemeMode::Light);
        assert_eq!(
            keyword(&rust),
            Some(SyntaxPalette::light().keyword),
            "the same theme again changes nothing"
        );

        let markdown_text = "[a](http://example.com)\n\nbody";
        let markdown =
            Document::open(2, &scratch.file("a.md", markdown_text), counter().1).expect("opens");
        let colors = |document: &Document| {
            let preview = document.markdown().expect("a preview");
            preview
                .ranges(
                    markdown_text,
                    &HighlightContext::new(markdown_text, markdown_text.len()..markdown_text.len()),
                )
                .iter()
                .filter_map(|run| run.style.color)
                .collect::<Vec<_>>()
        };
        let dark = colors(&markdown);
        markdown.set_theme(ThemeMode::Light);
        let light = colors(&markdown);
        assert!(!dark.is_empty() && !light.is_empty());
        assert_ne!(
            dark, light,
            "the preview is made again in the colors of the other theme"
        );
    }

    #[test]
    fn a_document_has_no_source_of_completions_until_one_is_set() {
        struct Nothing;
        impl Source for Nothing {
            fn request(&self, done: Box<dyn FnOnce(Option<crate::services::lsp::Answer>)>) {
                done(None);
            }
        }
        let scratch = Scratch::new("source");
        let document = Document::open(1, &scratch.file("s.rs", "x"), counter().1).expect("opens");
        assert!(document.completions().is_none());
        document.set_completions(Rc::new(Nothing));
        assert!(document.completions().is_some());
    }

    #[test]
    fn going_to_a_byte_puts_the_caret_there_asks_for_focus_and_changes_nothing() {
        let scratch = Scratch::new("go-to");
        // The first line has an accent, which is two bytes, so a byte is not a column.
        let text = "let \u{e9} = 1;\nlet x = 2;\n";
        let path = scratch.file("goto.rs", text);
        let (count, notify) = counter();
        let document = Document::open(1, &path, notify).expect("opens");
        assert!(
            document.focus.request().is_none(),
            "nothing asked for focus yet"
        );

        document.go_to(text.find('x').unwrap());
        assert_eq!(
            document.controller.cursor_position(),
            (2, 5),
            "the fifth column of the second line"
        );
        let selection = document.controller.value().selection();
        assert_eq!(
            selection.anchor(),
            selection.focus(),
            "a caret and not a selection"
        );
        assert!(
            document.focus.request().is_some(),
            "the editor is where the caret is to be seen and typed at"
        );
        assert!(!document.is_dirty(), "going somewhere is not an edit");
        assert_eq!(
            count.get(),
            0,
            "and does not tell anyone the document changed"
        );
        assert_eq!(document.controller.value().text(), text);
    }

    #[test]
    fn going_to_a_byte_asks_for_a_frame_so_that_the_text_area_scrolls_to_the_caret() {
        // The text area scrolls to a caret that moved in its next layout, and a layout is only done for a frame. A place
        // that an answer of the language server points at comes from no event, so nothing else asks for one: the caret
        // moved and the view stayed where it was.
        let scratch = Scratch::new("go-to-frame");
        let document = Document::open(
            1,
            &scratch.file("frame.rs", "fn a() {}\nfn b() {}\n"),
            counter().1,
        )
        .expect("opens");
        let asked = Rc::new(Cell::new(0));
        let count = Rc::clone(&asked);
        let previous =
            aimer::events::window::set_thread_redraw_requester(move || count.set(count.get() + 1));
        document.go_to(10);
        aimer::events::window::restore_thread_redraw_requester(previous);
        assert!(asked.get() > 0, "a frame is asked for");
    }

    #[test]
    fn showing_a_range_puts_the_caret_at_its_start_and_asks_for_it_to_be_shown_and_for_a_frame_and_the_focus()
     {
        let scratch = Scratch::new("show");
        let text = "let \u{e9} = 1;\nlet name = 2;\n";
        let (count, notify) = counter();
        let document = Document::open(1, &scratch.file("show.rs", text), notify).expect("opens");
        let name = text.find("name").unwrap();
        let asked = Rc::new(Cell::new(0));
        let count_frames = Rc::clone(&asked);
        let previous = aimer::events::window::set_thread_redraw_requester(move || {
            count_frames.set(count_frames.get() + 1)
        });
        document.show(name..name + 4);
        aimer::events::window::restore_thread_redraw_requester(previous);
        assert_eq!(
            document.controller.value().selection().range(),
            name..name,
            "a caret at the start of the name"
        );
        assert_eq!(
            document.controller.take_locate(),
            Some(name..name + 4),
            "and the name is to be shown"
        );
        assert!(
            document.focus.request().is_some(),
            "the caret is to be seen and typed at"
        );
        assert!(
            asked.get() > 0,
            "a frame is asked for, which is when the text area shows it"
        );
        assert!(
            !document.is_dirty() && count.get() == 0,
            "it is not an edit"
        );
        assert_eq!(document.controller.value().text(), text);
    }

    #[test]
    fn going_to_a_byte_that_is_past_the_end_or_inside_a_character_goes_to_the_end_or_the_start_of_it()
     {
        let scratch = Scratch::new("go-to-edges");
        let text = "a\u{e9}b\n";
        let document =
            Document::open(1, &scratch.file("edges.rs", text), counter().1).expect("opens");
        document.go_to(2);
        assert_eq!(
            document.controller.cursor_position(),
            (1, 2),
            "inside the accent is before it"
        );
        document.go_to(10_000);
        assert_eq!(
            document.controller.cursor_position(),
            (2, 1),
            "past the end is the end, after the last line break"
        );
        document.go_to(0);
        assert_eq!(document.controller.cursor_position(), (1, 1));
    }

    #[test]
    fn a_toml_file_is_toml_and_has_a_toml_highlighter() {
        assert_eq!(Language::of_path(Path::new("Cargo.toml")), Language::Toml);
        assert_eq!(Language::of_path(Path::new("a/B.TOML")), Language::Toml);
        assert_eq!(Language::Toml.label(), "TOML");
        assert_eq!(Language::Toml.line_comment(), Some("#"));
        let scratch = Scratch::new("toml-open");
        let path = scratch.file("Cargo.toml", "[package]\nname = \"a\"\n");
        let document = Document::open(1, &path, Rc::new(|| {})).expect("opens");
        assert!(document.syntax.is_some());
        assert!(document.is_cargo_manifest());
        let other = scratch.file("rustfmt.toml", "a = 1\n");
        let other = Document::open(2, &other, Rc::new(|| {})).expect("opens");
        assert!(!other.is_cargo_manifest());
    }

    #[test]
    fn a_file_is_in_the_language_its_extension_says() {
        assert_eq!(Language::of_path(Path::new("src/main.rs")), Language::Rust);
        assert_eq!(Language::of_path(Path::new("MAIN.RS")), Language::Rust);
        assert_eq!(
            Language::of_path(Path::new("notes.txt")),
            Language::PlainText
        );
        assert_eq!(
            Language::of_path(Path::new("Makefile")),
            Language::PlainText
        );
        assert_eq!(
            Language::of_path(Path::new("rs")),
            Language::PlainText,
            "a file called rs has no extension"
        );
    }

    #[test]
    fn a_file_opens_at_its_first_line_with_its_text_and_no_changes() {
        let scratch = Scratch::new("open");
        let path = scratch.file("hello.rs", "fn main() {}\nlet x = 1;\n");
        let (count, notify) = counter();
        let document = Document::open(7, &path, notify).expect("opens");
        assert_eq!(
            (document.id, document.name.as_str(), document.language),
            (7, "hello.rs", Language::Rust)
        );
        assert_eq!(
            document.controller.value().text(),
            "fn main() {}\nlet x = 1;\n"
        );
        assert_eq!(
            document.controller.cursor_position(),
            (1, 1),
            "at the start, not the end"
        );
        assert!(!document.is_dirty() && document.syntax.is_some());
        assert_eq!(count.get(), 0);

        let plain = Document::open(8, &scratch.file("notes.txt", "x"), counter().1).expect("opens");
        assert!(plain.syntax.is_none(), "a plain text file has no syntax");
    }

    #[test]
    fn a_file_that_is_not_text_or_is_not_there_is_not_opened() {
        let scratch = Scratch::new("bad");
        let binary = scratch.0.join("binary.rs");
        std::fs::write(&binary, [0xff, 0xfe, 0x00]).unwrap();
        assert!(Document::open(1, &binary, counter().1).is_err());
        assert!(Document::open(2, &scratch.0.join("missing.rs"), counter().1).is_err());
    }

    #[test]
    fn a_document_is_changed_while_its_text_differs_from_the_disk_and_says_so_when_that_flips() {
        let scratch = Scratch::new("dirty");
        let path = scratch.file("a.rs", "one");
        let (count, notify) = counter();
        let document = Document::open(1, &path, notify).unwrap();

        document.controller.select(0, 1);
        assert!(!document.is_dirty(), "moving the caret changes no text");
        assert_eq!(count.get(), 0);

        document.controller.set_text("one two");
        assert!(document.is_dirty());
        assert_eq!(count.get(), 1);
        document.controller.set_text("one two three");
        assert_eq!(count.get(), 1, "still changed: nothing to say");

        // Back to what it was on disk: unchanged again, however it got there.
        document.controller.set_text("one");
        assert!(!document.is_dirty());
        assert_eq!(count.get(), 2);
    }

    /// Writes `text` to `path` the way another program does: not the way the editor saves, and with a length that is not the file's.
    fn changed_elsewhere(path: &Path, text: &str) {
        std::fs::write(path, text).expect("another program writes the file");
    }

    #[test]
    fn the_file_is_the_same_until_something_else_changes_it_and_the_save_of_the_editor_is_not_a_change_by_something_else()
     {
        let scratch = Scratch::new("disk");
        let path = scratch.file("a.rs", "one");
        let document = Document::open(1, &path, counter().1).unwrap();
        assert_eq!(document.disk(), Disk::Same);
        document
            .controller
            .set_text("two, saved by the editor");
        document.save().expect("saves");
        assert_eq!(
            document.disk(),
            Disk::Same,
            "what the editor wrote is what it expects to find"
        );
        changed_elsewhere(&path, "three, written by another program");
        assert_eq!(document.disk(), Disk::Changed);
        assert_eq!(
            document.disk(),
            Disk::Changed,
            "and it stays changed until the editor has looked"
        );
        std::fs::remove_file(&path).unwrap();
        assert_eq!(document.disk(), Disk::Gone);
    }

    #[test]
    fn a_change_that_the_editor_has_seen_is_not_one_again_and_the_next_is() {
        let scratch = Scratch::new("seen");
        let path = scratch.file("a.rs", "one");
        let document = Document::open(1, &path, counter().1).unwrap();
        changed_elsewhere(&path, "two!");
        assert_eq!(document.disk(), Disk::Changed);
        document.acknowledge_disk();
        assert_eq!(document.disk(), Disk::Same);
        changed_elsewhere(&path, "three!!");
        assert_eq!(document.disk(), Disk::Changed);
        // A file that is gone and comes back is a change.
        std::fs::remove_file(&path).unwrap();
        assert_eq!(document.disk(), Disk::Gone);
        document.acknowledge_disk();
        assert_eq!(document.disk(), Disk::Same, "told once that it is gone");
        changed_elsewhere(&path, "back");
        assert_eq!(document.disk(), Disk::Changed);
    }

    #[test]
    fn reloading_puts_what_is_on_disk_in_the_text_keeps_the_caret_where_it_was_in_the_text_and_leaves_the_document_unchanged()
     {
        let scratch = Scratch::new("reload");
        let path = scratch.file("a.rs", "alpha\nbeta\ngamma\n");
        let (count, notify) = counter();
        let document = Document::open(1, &path, notify).unwrap();
        // The caret is in `gamma`, at its `m`: byte 14.
        document.controller.select(14, 14);
        changed_elsewhere(&path, "alpha\nBETA, longer\ngamma\n");
        assert_eq!(document.reload().expect("reads"), Reloaded::Replaced);
        assert_eq!(
            document.controller.value().text(),
            "alpha\nBETA, longer\ngamma\n"
        );
        assert!(!document.is_dirty());
        assert_eq!(document.disk(), Disk::Same);
        let caret = document
            .controller
            .value()
            .selection()
            .range()
            .start;
        assert_eq!(
            &document.controller.value().text()[caret..caret + 2],
            "ma",
            "still at the `m` of `gamma`, which moved with the text"
        );
        assert_eq!(count.get(), 0, "unchanged before and after: nothing to say");
    }

    #[test]
    fn a_caret_in_what_was_replaced_goes_to_the_start_of_what_replaced_it_and_one_before_it_stays()
    {
        let scratch = Scratch::new("caret");
        let path = scratch.file("a.rs", "alpha beta gamma");
        let document = Document::open(1, &path, counter().1).unwrap();
        document.controller.select(8, 8);
        changed_elsewhere(&path, "alpha BETA!! gamma");
        document.reload().unwrap();
        assert_eq!(
            document.controller.value().selection().range(),
            6..6,
            "in `beta`, which was replaced"
        );
        document.controller.select(2, 2);
        changed_elsewhere(&path, "alpha BETA!! gamma, too");
        document.reload().unwrap();
        assert_eq!(
            document.controller.value().selection().range(),
            2..2,
            "before it"
        );
    }

    #[test]
    fn a_reload_of_what_is_the_same_text_changes_nothing_and_takes_note_of_the_file() {
        let scratch = Scratch::new("same-text");
        let path = scratch.file("a.rs", "one");
        let document = Document::open(1, &path, counter().1).unwrap();
        changed_elsewhere(&path, "one");
        let before = document.controller.revision();
        assert_eq!(document.reload().unwrap(), Reloaded::Unchanged);
        assert_eq!(
            document.controller.revision(),
            before,
            "the text was not touched"
        );
        assert_eq!(document.disk(), Disk::Same);
    }

    #[test]
    fn a_reload_of_a_document_that_was_changed_makes_it_the_file_and_says_so() {
        let scratch = Scratch::new("reload-dirty");
        let path = scratch.file("a.rs", "one");
        let (count, notify) = counter();
        let document = Document::open(1, &path, notify).unwrap();
        document.controller.set_text("mine");
        assert!(document.is_dirty());
        let told = count.get();
        changed_elsewhere(&path, "theirs");
        document.reload().unwrap();
        assert!(!document.is_dirty());
        assert_eq!(document.controller.value().text(), "theirs");
        assert!(count.get() > told, "it became unchanged, which is said");
        assert!(
            document.controller.undo(),
            "and a reload can be taken back, which is a text that is not the file's, and so changed"
        );
        assert!(document.is_dirty());
    }

    #[test]
    fn a_file_that_cannot_be_read_as_text_is_not_reloaded_and_the_text_is_as_it_was() {
        let scratch = Scratch::new("reload-binary");
        let path = scratch.file("a.rs", "one");
        let document = Document::open(1, &path, counter().1).unwrap();
        std::fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        assert!(document.reload().is_err());
        assert_eq!(document.controller.value().text(), "one");
        std::fs::remove_file(&path).unwrap();
        assert!(document.reload().is_err(), "nor one that is not there");
        assert!(!document.is_dirty());
    }

    #[test]
    fn what_watches_for_edits_is_told_of_each_change_of_the_text_and_not_of_a_move_of_the_caret_or_of_a_save()
     {
        let scratch = Scratch::new("edits");
        let document = Document::open(1, &scratch.file("a.rs", "one"), counter().1).unwrap();
        let (edits, hook) = counter();
        document.on_edit(hook);
        document.controller.select(0, 2);
        assert_eq!(edits.get(), 0, "a caret is not an edit");
        document.controller.set_text("one two");
        assert_eq!(edits.get(), 1);
        document.controller.set_text("one two three");
        assert_eq!(
            edits.get(),
            2,
            "each, though the document was changed already"
        );
        document.controller.select(1, 1);
        assert_eq!(edits.get(), 2);
        document.save().expect("saves");
        assert_eq!(edits.get(), 2, "a save changes no text");
        assert!(document.controller.undo());
        assert_eq!(edits.get(), 3, "taking an edit back is one");
        // What was said can be said again: the hook that is set last is the one that is told.
        let (later, hook) = counter();
        document.on_edit(hook);
        document.controller.set_text("x");
        assert_eq!((edits.get(), later.get()), (3, 1));
    }

    #[test]
    fn saving_writes_the_text_to_the_file_and_makes_the_document_unchanged() {
        let scratch = Scratch::new("save");
        let path = scratch.file("a.rs", "old\n");
        let (count, notify) = counter();
        let document = Document::open(1, &path, notify).unwrap();
        document
            .controller
            .set_text("new text, caf\u{e9}\n");
        assert!(document.is_dirty());

        document.save().expect("saves");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "new text, caf\u{e9}\n"
        );
        assert!(!document.is_dirty());
        assert_eq!(count.get(), 2, "it became changed, then unchanged");
        // No temporary file is left beside it.
        let left: Vec<_> = std::fs::read_dir(&scratch.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left.len(), 1, "{left:?}");

        // A later edit is compared with what was saved, not with what was opened.
        document.controller.set_text("old\n");
        assert!(document.is_dirty());
    }

    #[test]
    fn saving_a_long_text_writes_all_of_it() {
        let scratch = Scratch::new("long");
        let text = "a line of code with caf\u{e9}, long enough to be many chunks\n".repeat(20_000);
        let path = scratch.file("big.rs", &text);
        let document = Document::open(1, &path, counter().1).unwrap();
        document.controller.select(0, 0);
        document
            .controller
            .set_text(format!("// header\n{text}"));
        document.save().expect("saves");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            format!("// header\n{text}")
        );
    }

    #[cfg(unix)]
    #[test]
    fn saving_keeps_the_permissions_and_writes_through_a_link() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new("perm");
        let target = scratch.file("real.sh", "echo\n");
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
        let link = scratch.0.join("link.sh");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let document = Document::open(1, &link, counter().1).unwrap();
        document.controller.set_text("echo changed\n");
        document.save().expect("saves");
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "echo changed\n",
            "the file the link points to"
        );
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "and the link is still a link"
        );
        assert_eq!(
            std::fs::metadata(&target)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
    }

    #[test]
    fn a_save_that_fails_says_why_and_leaves_the_document_changed_and_the_file_alone() {
        let scratch = Scratch::new("fail");
        let path = scratch.file("sub/a.rs", "kept");
        let document = Document::open(1, &path, counter().1).unwrap();
        document.controller.set_text("lost?");
        std::fs::remove_dir_all(scratch.0.join("sub")).unwrap();
        assert!(document.save().is_err());
        assert!(document.is_dirty(), "what was not saved is still unsaved");
    }
}
