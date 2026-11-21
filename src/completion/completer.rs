//! Completion for one document: opens the menu as the text is typed, moves it with the keys, and puts the
//! chosen item in the text.
//!
//! The items come from the language server. It is asked when a word is begun, or after a `.` or a `::`, and
//! not again as the word grows: the answer is kept, and narrowed by each letter typed, which costs the items that
//! are left and not a trip to the server. Where the server cannot be asked, the keywords of the language are what
//! is offered.

use super::call::Call;
use super::keywords::rust_keywords;
use super::menu::{Menu, MenuCell};
use super::rank::rank;
use super::word::{Word, word_before};
use crate::document::Language;
use crate::lsp::{Answer, Item};
use aimer::events::element::NamedKey;
use aimer::input::input::TextSnapshot;
use aimer::input::{TextEditingController, TextEditingValue};
use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Range;
use std::rc::{Rc, Weak};
use std::time::Duration;

/// How long a server is given to answer before the keywords are shown in its place, until it does. The first answer
/// of a server that is still loading the project may be a long way off, and a menu that is not there for that long
/// looks as if there were none.
const STALL_AFTER: Duration = Duration::from_millis(250);

/// Where the items come from.
pub trait Source {
    /// Asks what could be put at the caret, and calls `done` with the items, or with none where it cannot say.
    /// `done` is not called if the answer is no longer wanted.
    fn request(&self, done: Box<dyn FnOnce(Option<Answer>)>);

    /// Asks for the other edits that putting `item` in asks for, such as the line that imports it, which a server
    /// that offers what is not in scope says when the item is chosen and not before. `done` is called with them, or
    /// with none where they could not be had. The default has none to give.
    fn resolve(&self, item: &Item, done: Box<dyn FnOnce(Option<Vec<(Range<usize>, String)>>)>) {
        let _ = item;
        done(None);
    }
}

/// An answer that is kept, and narrowed as the word grows.
struct Cache {
    list: Rc<Vec<Item>>,
    /// Where the word it is an answer for starts.
    start: usize,
    /// The word as it was when the answer was asked for: it holds for any word that begins with it.
    requested: String,
    /// Whether the server says that more of the word would give other items, so that the answer holds for no
    /// other word.
    incomplete: bool,
    /// The items that fitted the last word, and the word: what a longer word is looked for in.
    view: Option<(String, Vec<u32>)>,
    /// Tells this answer from another, so that what is said of an item of one is not taken for an item of the other.
    serial: u64,
    /// The other edits of the items that were asked for them, by the number of the item in the list.
    resolved: HashMap<u32, Vec<(Range<usize>, String)>>,
}

pub struct Completer {
    language: Language,
    controller: TextEditingController,
    menu: Rc<MenuCell>,
    /// The value as it was when the menu was last worked out: a change of the caret alone leaves its
    /// text the same, and a change of the text does not. It is a value and not a snapshot because a
    /// short text makes its snapshot again for each copy of the value, so two snapshots of the same
    /// text are not the same object; `same_text` is a pointer or length check for a caret move.
    last: RefCell<TextEditingValue>,
    /// Set while an item is put in, so that the edit does not open the menu again.
    accepting: Cell<bool>,
    source: Option<Rc<dyn Source>>,
    /// The keywords, which are the same list every time, so that a menu of them shares it.
    keywords: Rc<Vec<Item>>,
    cache: RefCell<Option<Cache>>,
    /// Counts the answers that were kept.
    serials: Cell<u64>,
    /// Counts the times the menu has to be thought of again, so that an answer that comes after that is known to
    /// be for what is not there any more.
    generation: Cell<u64>,
    me: Weak<Completer>,
}

impl Completer {
    pub fn new(
        language: Language,
        controller: TextEditingController,
        source: Option<Rc<dyn Source>>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|me: &Weak<Completer>| {
            let menu: Rc<MenuCell> = Rc::default();
            let picked = me.clone();
            menu.on_pick(Rc::new(move |number| {
                if let Some(completer) = picked.upgrade() {
                    completer.pick(number);
                }
            }));
            let dismissed = me.clone();
            menu.on_dismiss(Rc::new(move || {
                if let Some(completer) = dismissed.upgrade() {
                    completer.close();
                }
            }));
            Self {
                language,
                last: RefCell::new(controller.value()),
                controller,
                menu,
                accepting: Cell::new(false),
                // Only Rust has a source: the server is run for it and for no other language.
                source: source.filter(|_| language == Language::Rust),
                keywords: Rc::new(if language == Language::Rust {
                    rust_keywords()
                } else {
                    Vec::new()
                }),
                cache: RefCell::new(None),
                serials: Cell::new(0),
                generation: Cell::new(0),
                me: me.clone(),
            }
        })
    }

    /// The menu, for what draws it.
    pub fn menu(&self) -> &Rc<MenuCell> {
        &self.menu
    }

    /// Works out the menu again. Call it whenever the controller changes: the menu follows what was
    /// typed, and goes away when only the caret moved or a range is selected.
    pub fn refresh(&self) {
        // A language with nothing to offer reads nothing: not even the snapshot, which for a short
        // text is a copy of all of it.
        if self.language != Language::Rust {
            return;
        }
        let value = self.controller.value();
        let typed = !value.same_text(&self.last.borrow());
        *self.last.borrow_mut() = value.clone();
        let selection = value.selection().range();
        if !typed || self.accepting.get() || !selection.is_empty() {
            self.close();
            return;
        }
        let Some(word) =
            word_before(&value.snapshot(), selection.start).filter(Word::is_completable)
        else {
            self.close();
            return;
        };
        let (usable, incomplete) = match self.cache.borrow().as_ref() {
            Some(cache) => (
                cache.start == word.start && word.text.starts_with(&cache.requested),
                cache.incomplete,
            ),
            None => (false, false),
        };
        // What was answered for the word as it was holds for the word as it is, if it has only grown: it is shown
        // narrowed at once, and where the server says that its answer is cut short, it is asked again.
        if usable {
            self.show(&word.text);
        } else {
            // What was shown is for another word, and is not shown while the answer for this one is waited for.
            *self.cache.borrow_mut() = None;
            self.menu.set(None);
        }
        if !usable || incomplete {
            self.ask(word);
        }
    }

    /// Shuts the menu and forgets what was asked: the word is not one that is completed, or the caret left it.
    fn close(&self) {
        *self.cache.borrow_mut() = None;
        self.generation.set(self.generation.get() + 1);
        self.menu.set(None);
    }

    /// Where the items for `word` are to come from: the server, and the keywords where it cannot say.
    fn ask(&self, word: Word) {
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        let Some(source) = &self.source else {
            self.show_keywords(&word);
            return;
        };
        let me = self.me.clone();
        // The keywords are shown if there is nothing by then. Where there is no runtime there is no moment, and
        // the server alone is waited for.
        if let Some(venus) = aimer::Venus::current() {
            let (waited, me, word) = (venus.sleep(STALL_AFTER), self.me.clone(), word.clone());
            venus.spawn(async move {
                // A runtime that has no timers answers at once, which is a moment of no length.
                let _ = waited.await;
                if let Some(me) = me.upgrade() {
                    me.stalled(generation, &word);
                }
            });
        }
        source.request(Box::new(move |answer| {
            if let Some(me) = me.upgrade() {
                me.arrived(generation, word, answer);
            }
        }));
    }

    /// The server has not answered the request made for `word` at `generation` in the moment it was given: the
    /// keywords are shown, if the word is still the one that is typed and there is nothing shown for it.
    fn stalled(&self, generation: u64, word: &Word) {
        if generation == self.generation.get() && !self.menu.is_open() {
            self.show_keywords(word);
        }
    }

    /// The answer to the request made for `word` at `generation`.
    fn arrived(&self, generation: u64, word: Word, answer: Option<Answer>) {
        // The word has been typed on, or the caret has left it, and what is wanted is another answer.
        if generation != self.generation.get() {
            return;
        }
        let Some(answer) = answer else {
            self.show_keywords(&word);
            return;
        };
        // What the items replace begins where the word does. The server's own range says, and a server whose
        // idea of the word is not ours is not one that can be filtered by it.
        let start = answer
            .items
            .iter()
            .find_map(|item| {
                item.edit
                    .range
                    .as_ref()
                    .map(|range| range.start)
            })
            .unwrap_or(word.start);
        if answer.items.is_empty() || start != word.start {
            *self.cache.borrow_mut() = None;
            self.menu.set(None);
            return;
        }
        self.serials.set(self.serials.get() + 1);
        *self.cache.borrow_mut() = Some(Cache {
            list: Rc::new(answer.items),
            start,
            requested: word.text.clone(),
            incomplete: answer.incomplete,
            view: None,
            serial: self.serials.get(),
            resolved: HashMap::new(),
        });
        self.show(&word.text);
    }

    /// Shows the items of the answer that is kept that fit `word`.
    fn show(&self, word: &str) {
        let menu = {
            let mut cache = self.cache.borrow_mut();
            let Some(cache) = cache.as_mut() else {
                return;
            };
            // A word that has grown fits what fitted it before and nothing else.
            let within = cache
                .view
                .as_ref()
                .filter(|(seen, _)| word.starts_with(seen.as_str()))
                .map(|(_, view)| view.as_slice());
            let mut view = rank(&cache.list, within, word, true);
            cache.view = Some((word.to_owned(), view.clone()));
            // The word as it is typed is not offered where it is all there is: there is nothing to add.
            if let [only] = view[..] {
                if changes_nothing(&cache.list[only as usize], word) {
                    view.clear();
                }
            }
            Menu::new(Rc::clone(&cache.list), view, cache.start, word)
        };
        self.menu.set(menu);
        self.resolve_chosen();
    }

    /// Asks for the other edits of the item that is chosen, if it has some to give that are not had, so that they are
    /// there when it is put in: a moment after it is chosen, which is long before a key is pressed to put it in.
    fn resolve_chosen(&self) {
        let Some((index, item)) = self
            .menu
            .with(|menu| menu.map(|menu| (menu.chosen_index(), menu.chosen().clone())))
        else {
            return;
        };
        let (Some(source), Some(_)) = (&self.source, &item.resolve) else {
            return;
        };
        let serial = match self.cache.borrow().as_ref() {
            Some(cache) if !cache.resolved.contains_key(&index) => cache.serial,
            _ => return,
        };
        let me = self.me.clone();
        source.resolve(
            &item,
            Box::new(move |edits| {
                if let Some(me) = me.upgrade() {
                    me.resolved(serial, index, edits.unwrap_or_default());
                }
            }),
        );
    }

    /// The other edits of the item `index` of the answer `serial`, which is not the answer that is kept if the word
    /// has been typed on since.
    fn resolved(&self, serial: u64, index: u32, edits: Vec<(Range<usize>, String)>) {
        if let Some(cache) = self
            .cache
            .borrow_mut()
            .as_mut()
            .filter(|cache| cache.serial == serial)
        {
            cache.resolved.insert(index, edits);
        }
    }

    fn show_keywords(&self, word: &Word) {
        let mut view = rank(&self.keywords, None, &word.text, false);
        if let [only] = view[..] {
            if changes_nothing(&self.keywords[only as usize], &word.text) {
                view.clear();
            }
        }
        // Keywords are said of a word that is begun: after a `.` there is no keyword to say.
        let menu = if word.text.is_empty() {
            None
        } else {
            Menu::new(Rc::clone(&self.keywords), view, word.start, &word.text)
        };
        self.menu.set(menu);
    }

    /// Takes a key while the menu is showing: the arrows move in it, Return and Tab insert the item,
    /// Escape closes it. Answers whether the key was taken, which keeps it from the text.
    pub fn handle_key(&self, key: &NamedKey) -> bool {
        if !self.menu.is_open() {
            return false;
        }
        match key {
            NamedKey::ArrowDown => self.menu.step(true),
            NamedKey::ArrowUp => self.menu.step(false),
            NamedKey::PageDown => self.menu.page(true),
            NamedKey::PageUp => self.menu.page(false),
            NamedKey::Escape => self.close(),
            // A word that is typed whole, and is the item that is chosen, is not completed by Return: it is what
            // Return would have been without the menu, which is a new line.
            NamedKey::Enter
                if self.menu.with(|menu| {
                    menu.is_some_and(|menu| changes_nothing(menu.chosen(), menu.word()))
                }) =>
            {
                self.close();
                return false;
            }
            NamedKey::Enter | NamedKey::Tab => self.accept(),
            _ => return false,
        }
        // The item that is chosen may be another.
        self.resolve_chosen();
        true
    }

    /// Picks the item `number` of the menu, as a click on its row does.
    fn pick(&self, number: usize) {
        self.menu.change(|menu| menu.select(number));
        self.accept();
    }

    /// Puts the chosen item in place of the word before the caret, as one edit that undo takes back, with the other
    /// changes it asks for, such as the line that imports it.
    pub fn accept(&self) {
        let Some(menu) = self.menu.take() else { return };
        let caret = self.controller.value().selection().range().end;
        let item = menu.chosen();
        let start = menu.start();
        // What the server has said of the item since it was chosen, if it has said it by now: an item that is put in
        // before that has the edits it came with, which are not the line that imports it.
        let resolved = self
            .cache
            .borrow()
            .as_ref()
            .and_then(|cache| {
                cache
                    .resolved
                    .get(&menu.chosen_index())
                    .cloned()
            })
            .unwrap_or_default();
        // The other changes are about the text as it was asked about, in which what is before the word is as it is
        // now. One that is not wholly before it is not known to be where it was, and is not made.
        let mut edits: Vec<(Range<usize>, &str)> = item
            .edit
            .extra
            .iter()
            .chain(&resolved)
            .filter(|(range, _)| range.end <= start)
            .map(|(range, text)| (range.clone(), text.as_str()))
            .collect();
        // A function is called: its brackets are put after it, and the caret goes into them where there is an argument to type.
        let snapshot = self.controller.snapshot();
        let call = Call::parentheses(item, snapshot.byte_at(caret), &line_start(&snapshot, start));
        let text: Cow<str> = match call {
            Some(_) => Cow::Owned(format!("{}()", item.edit.text)),
            None => Cow::Borrowed(item.edit.text.as_str()),
        };
        // Those begin where the word does or before it, so the word's is the last of them in the text.
        let word_is = edits.len();
        edits.push((start..caret, text.as_ref()));
        self.accepting.set(true);
        if !self.controller.replace_ranges(&edits, word_is) {
            // Changes that overlap are not made, and the word still is.
            self.controller
                .replace_range(start..caret, &text);
        }
        if call == Some(true) {
            // Between the brackets: the edit left the caret after them, which are the last two bytes of the word.
            let end = self.controller.value().selection().range().end;
            self.controller.select_bytes(end - 1, end - 1);
        }
        self.accepting.set(false);
        self.close();
    }
}

/// The start of the line that `at` is on, up to `at`, as far as [`Call::IMPORT_LOOKBEHIND`] bytes of it: what tells that the
/// word at `at` is in an import.
fn line_start(text: &TextSnapshot, at: usize) -> String {
    let line = text.line_range(text.byte_to_line(at.min(text.len())));
    let end = at.min(line.start + Call::IMPORT_LOOKBEHIND);
    let bytes: Vec<u8> = (line.start..end)
        .map_while(|byte| text.byte_at(byte))
        .collect();
    // A cut in the middle of a character is only a character that is not an import.
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Whether putting `item` in place of `word` would do nothing: it is the word, and there is nothing else it asks for.
fn changes_nothing(item: &Item, word: &str) -> bool {
    item.edit.text == word && item.edit.extra.is_empty() && item.resolve.is_none()
}

#[cfg(test)]
mod tests;
