//! A file open in the editor: its text, its caret, its syntax, and whether it has changed.

use std::cell::{Cell, RefCell};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::completion::Source;
use aimer::FocusNode;
use aimer::input::{ControllerListener, TextEditingController, TextEditingValue};

use aimer::MarkdownLivePreview;

use crate::syntax::{SyntaxHighlighter, SyntaxPalette};

/// What a file is written in, which decides how it is colored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Rust,
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
            Some(extension) if extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown") => Self::Markdown,
            _ => Self::PlainText,
        }
    }

    /// The name shown in the status bar.
    pub fn label(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::Markdown => "Markdown",
            Self::PlainText => "Plain Text",
        }
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
    /// The live preview of a markdown file, made once for the same reason: it keeps the parse.
    pub markdown: Option<MarkdownLivePreview>,
    /// The text as it is on disk: what the document is compared with to know it has changed.
    saved: Rc<RefCell<TextEditingValue>>,
    dirty: Rc<Cell<bool>>,
    /// Called when the document becomes changed or unchanged, so what shows it can be redrawn.
    notify: Rc<dyn Fn()>,
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
    /// Opens the file at `path`, or says why it cannot be read. A file that is not UTF-8 is not
    /// opened: the editor holds text. `notify` is called whenever the document becomes changed or
    /// unchanged, which is not on every keystroke.
    pub fn open(id: u64, path: &Path, notify: Rc<dyn Fn()>) -> io::Result<Rc<Self>> {
        let text = std::fs::read_to_string(path)?;
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| {
                name.to_string_lossy()
                    .into_owned()
            },
        );
        let language = Language::of_path(path);
        let crlf = text
            .find('\n')
            .is_some_and(|at| at > 0 && text.as_bytes()[at - 1] == b'\r');
        // The text goes straight into the editor's buffer, without a copy made on the way. A file
        // opens at its first line, and a controller made with text starts at its end.
        let controller = TextEditingController::with_str(&text);
        controller.select(0, 0);
        let saved = Rc::new(RefCell::new(controller.value()));
        let dirty = Rc::new(Cell::new(false));
        let listener = {
            let (controller, saved, dirty, notify) = (
                controller.clone(),
                Rc::clone(&saved),
                Rc::clone(&dirty),
                Rc::clone(&notify),
            );
            controller
                .clone()
                .listen(move |_| {
                    // Moving the caret changes the value but not the text: only the text counts.
                    let changed = !controller
                        .value()
                        .same_text(&saved.borrow());
                    if dirty.replace(changed) != changed {
                        notify();
                    }
                })
        };
        let syntax = match language {
            Language::Rust => Some(SyntaxHighlighter::rust(SyntaxPalette::dark())),
            Language::Markdown | Language::PlainText => None,
        };
        let markdown = (language == Language::Markdown).then(crate::markdown::preview);
        Ok(Rc::new(Self {
            id,
            name,
            path: path.to_path_buf(),
            language,
            crlf,
            controller,
            focus: FocusNode::new(),
            completions: RefCell::new(None),
            syntax,
            markdown,
            saved,
            dirty,
            notify,
            _listener: listener,
        }))
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
    let name = path.file_name().map_or_else(
        || "file".into(),
        |name| {
            name.to_string_lossy()
                .into_owned()
        },
    );
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
    use crate::tree::Scratch;

    fn counter() -> (Rc<Cell<usize>>, Rc<dyn Fn()>) {
        let count = Rc::new(Cell::new(0));
        let counted = Rc::clone(&count);
        (count, Rc::new(move || counted.set(counted.get() + 1)))
    }

    #[test]
    fn markdown_files_are_markdown_and_have_a_preview_and_no_syntax() {
        for name in ["README.md", "notes.MD", "doc.markdown"] {
            assert_eq!(Language::of_path(Path::new(name)), Language::Markdown, "{name}");
        }
        let scratch = Scratch::new("markdown");
        let (_, notify) = counter();
        let document = Document::open(3, &scratch.file("a.md", "# Title\n"), notify).expect("opens");
        assert_eq!(document.language, Language::Markdown);
        assert_eq!(document.language.label(), "Markdown");
        assert!(document.markdown.is_some() && document.syntax.is_none());
        let rust = Document::open(4, &scratch.file("a.rs", "fn a() {}\n"), counter().1).expect("opens");
        assert!(rust.markdown.is_none());
    }

    #[test]
    fn a_document_has_no_source_of_completions_until_one_is_set() {
        struct Nothing;
        impl Source for Nothing {
            fn request(&self, done: Box<dyn FnOnce(Option<crate::lsp::Answer>)>) {
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
        assert!(document.focus.request().is_none(), "nothing asked for focus yet");

        document.go_to(text.find('x').unwrap());
        assert_eq!(document.controller.cursor_position(), (2, 5), "the fifth column of the second line");
        let selection = document.controller.value().selection();
        assert_eq!(selection.anchor(), selection.focus(), "a caret and not a selection");
        assert!(document.focus.request().is_some(), "the editor is where the caret is to be seen and typed at");
        assert!(!document.is_dirty(), "going somewhere is not an edit");
        assert_eq!(count.get(), 0, "and does not tell anyone the document changed");
        assert_eq!(document.controller.value().text(), text);
    }

    #[test]
    fn going_to_a_byte_asks_for_a_frame_so_that_the_text_area_scrolls_to_the_caret() {
        // The text area scrolls to a caret that moved in its next layout, and a layout is only done for a frame. A place
        // that an answer of the language server points at comes from no event, so nothing else asks for one: the caret
        // moved and the view stayed where it was.
        let scratch = Scratch::new("go-to-frame");
        let document = Document::open(1, &scratch.file("frame.rs", "fn a() {}\nfn b() {}\n"), counter().1).expect("opens");
        let asked = Rc::new(Cell::new(0));
        let count = Rc::clone(&asked);
        let previous = aimer::events::window::set_thread_redraw_requester(move || count.set(count.get() + 1));
        document.go_to(10);
        aimer::events::window::restore_thread_redraw_requester(previous);
        assert!(asked.get() > 0, "a frame is asked for");
    }

    #[test]
    fn showing_a_range_puts_the_caret_at_its_start_and_asks_for_it_to_be_shown_and_for_a_frame_and_the_focus() {
        let scratch = Scratch::new("show");
        let text = "let \u{e9} = 1;\nlet name = 2;\n";
        let (count, notify) = counter();
        let document = Document::open(1, &scratch.file("show.rs", text), notify).expect("opens");
        let name = text.find("name").unwrap();
        let asked = Rc::new(Cell::new(0));
        let count_frames = Rc::clone(&asked);
        let previous = aimer::events::window::set_thread_redraw_requester(move || count_frames.set(count_frames.get() + 1));
        document.show(name..name + 4);
        aimer::events::window::restore_thread_redraw_requester(previous);
        assert_eq!(document.controller.value().selection().range(), name..name, "a caret at the start of the name");
        assert_eq!(document.controller.take_locate(), Some(name..name + 4), "and the name is to be shown");
        assert!(document.focus.request().is_some(), "the caret is to be seen and typed at");
        assert!(asked.get() > 0, "a frame is asked for, which is when the text area shows it");
        assert!(!document.is_dirty() && count.get() == 0, "it is not an edit");
        assert_eq!(document.controller.value().text(), text);
    }

    #[test]
    fn going_to_a_byte_that_is_past_the_end_or_inside_a_character_goes_to_the_end_or_the_start_of_it() {
        let scratch = Scratch::new("go-to-edges");
        let text = "a\u{e9}b\n";
        let document = Document::open(1, &scratch.file("edges.rs", text), counter().1).expect("opens");
        document.go_to(2);
        assert_eq!(document.controller.cursor_position(), (1, 2), "inside the accent is before it");
        document.go_to(10_000);
        assert_eq!(document.controller.cursor_position(), (2, 1), "past the end is the end, after the last line break");
        document.go_to(0);
        assert_eq!(document.controller.cursor_position(), (1, 1));
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
            document
                .controller
                .value()
                .text(),
            "fn main() {}\nlet x = 1;\n"
        );
        assert_eq!(
            document
                .controller
                .cursor_position(),
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

        document
            .controller
            .set_text("one two");
        assert!(document.is_dirty());
        assert_eq!(count.get(), 1);
        document
            .controller
            .set_text("one two three");
        assert_eq!(count.get(), 1, "still changed: nothing to say");

        // Back to what it was on disk: unchanged again, however it got there.
        document
            .controller
            .set_text("one");
        assert!(!document.is_dirty());
        assert_eq!(count.get(), 2);
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
        document
            .controller
            .set_text("old\n");
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
        document
            .controller
            .set_text("echo changed\n");
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
        document
            .controller
            .set_text("lost?");
        std::fs::remove_dir_all(scratch.0.join("sub")).unwrap();
        assert!(document.save().is_err());
        assert!(document.is_dirty(), "what was not saved is still unsaved");
    }
}
