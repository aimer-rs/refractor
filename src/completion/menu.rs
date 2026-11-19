//! The menu that is on screen: the items that fit the word, which one is selected, and which rows of them are
//! shown. A list may have thousands of items; only a window of them is ever drawn.

use crate::lsp::Item;
use std::cell::RefCell;
use std::rc::Rc;

/// How many rows the menu shows at once.
pub const WINDOW: usize = 10;

/// A row of the window.
pub struct Row<'a> {
    /// Which of the items that fit this is, from zero: what a click on the row picks.
    pub number: usize,
    pub item: &'a Item,
    /// How many bytes of the label, from its start, are the word typed: what the row marks.
    pub typed: usize,
    pub selected: bool,
}

/// A completion on screen.
///
/// The items are shared with what made them, so that a keystroke that only narrows the menu makes a list of
/// numbers and not of items. Two menus are equal when they show the same.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    list: Rc<Vec<Item>>,
    /// The numbers of the items that fit, best first. Never empty.
    view: Vec<u32>,
    /// Which of the view is selected.
    selected: usize,
    /// Which of the view is the first of the window.
    scroll: usize,
    /// The byte where the word being completed starts: the range from here to the caret is what an item replaces.
    start: usize,
    /// The word typed.
    word: String,
}

impl Menu {
    /// A menu of the items `view` numbers, the best selected. None where there are none.
    pub fn new(list: Rc<Vec<Item>>, view: Vec<u32>, start: usize, word: &str) -> Option<Self> {
        (!view.is_empty()).then(|| Self { list, view, selected: 0, scroll: 0, start, word: word.to_owned() })
    }

    /// How many items fit the word.
    pub fn len(&self) -> usize {
        self.view.len()
    }

    /// Which of them is selected, from zero.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Where the word being completed starts, in bytes.
    pub fn start(&self) -> usize {
        self.start
    }

    /// The word typed.
    pub fn word(&self) -> &str {
        &self.word
    }

    /// Selects the item `number` of those that fit. A number past the last is the last.
    pub fn select(&mut self, number: usize) {
        self.selected = number.min(self.view.len() - 1);
        self.show_selected();
    }

    /// Which of the list the item that Return inserts is.
    pub fn chosen_index(&self) -> u32 {
        self.view[self.selected]
    }

    /// The item Return inserts.
    pub fn chosen(&self) -> &Item {
        &self.list[self.view[self.selected] as usize]
    }

    /// The rows that are shown, from the top of the window.
    pub fn rows(&self) -> impl Iterator<Item = Row<'_>> {
        let end = (self.scroll + WINDOW).min(self.view.len());
        (self.scroll..end).map(move |at| {
            let item = &self.list[self.view[at] as usize];
            // Only a label that begins with the word, but for its case, has a part of it to mark.
            let typed = item.label.get(..self.word.len()).filter(|begin| begin.eq_ignore_ascii_case(&self.word)).map_or(0, str::len);
            Row { number: at, item, typed, selected: at == self.selected }
        })
    }

    /// Moves the selection one item, past the last to the first and past the first to the last.
    pub fn step(&mut self, forward: bool) {
        let count = self.view.len();
        self.selected = if forward { (self.selected + 1) % count } else { (self.selected + count - 1) % count };
        self.show_selected();
    }

    /// Moves the selection a window of items, and stops at the first and the last.
    pub fn page(&mut self, forward: bool) {
        self.selected = if forward { (self.selected + WINDOW).min(self.view.len() - 1) } else { self.selected.saturating_sub(WINDOW) };
        self.show_selected();
    }

    /// Scrolls the window the least that shows the selected row.
    fn show_selected(&mut self) {
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + WINDOW {
            self.scroll = self.selected + 1 - WINDOW;
        }
    }
}

/// The menu shared between the editor, which decides when there is one, and the widget that draws
/// it. The widget is kept by the text area and not rebuilt with the pane, so it is told to draw
/// again when the menu changes.
#[derive(Default)]
pub struct MenuCell {
    menu: RefCell<Option<Menu>>,
    redraw: RefCell<Option<Rc<dyn Fn()>>>,
    /// What is called with the number of the row that was clicked.
    picker: RefCell<Option<Rc<dyn Fn(usize)>>>,
    /// What is called when the menu is to go because of where it is: the editor lost focus or changed size.
    dismisser: RefCell<Option<Rc<dyn Fn()>>>,
}

impl MenuCell {
    /// Whether a menu is showing.
    pub fn is_open(&self) -> bool {
        self.menu.borrow().is_some()
    }

    /// Reads the menu, which is `None` when none is showing.
    pub fn with<T>(&self, read: impl FnOnce(Option<&Menu>) -> T) -> T {
        read(self.menu.borrow().as_ref())
    }

    /// Changes the menu, and tells what draws it when it is not the same: typing on in a word that
    /// still offers the same items asks for nothing.
    pub fn set(&self, menu: Option<Menu>) {
        if *self.menu.borrow() == menu {
            return;
        }
        *self.menu.borrow_mut() = menu;
        // Cloned out of the cell first: what draws again may read the menu.
        let redraw = self.redraw.borrow().clone();
        if let Some(redraw) = redraw {
            redraw();
        }
    }

    /// Changes what is selected in the menu that is showing.
    pub fn step(&self, forward: bool) {
        self.change(|menu| menu.step(forward));
    }

    /// Moves the selection of the menu that is showing a window of items.
    pub fn page(&self, forward: bool) {
        self.change(|menu| menu.page(forward));
    }

    /// Changes the menu that is showing, as `change` says.
    pub fn change(&self, change: impl FnOnce(&mut Menu)) {
        let mut menu = self.menu.borrow().clone();
        if let Some(menu) = menu.as_mut() {
            change(menu);
        }
        self.set(menu);
    }

    /// Takes the menu away, returning it: what accepting an item does.
    pub fn take(&self) -> Option<Menu> {
        let menu = self.menu.borrow().clone();
        self.set(None);
        menu
    }

    /// Sets what is called with the number of a row that is clicked, replacing the one before.
    pub fn on_pick(&self, picker: Rc<dyn Fn(usize)>) {
        *self.picker.borrow_mut() = Some(picker);
    }

    /// Sets what is called when the menu is dismissed, replacing the one before.
    pub fn on_dismiss(&self, dismisser: Rc<dyn Fn()>) {
        *self.dismisser.borrow_mut() = Some(dismisser);
    }

    /// Says that the menu is to go: the editor has lost focus, or is not the size it was when the menu was put where it is.
    pub fn dismiss(&self) {
        // Cloned out of the cell first: what is called closes the menu.
        let dismisser = self.dismisser.borrow().clone();
        match dismisser {
            Some(dismisser) => dismisser(),
            None => self.set(None),
        }
    }

    /// Says that the row `number` was clicked.
    pub fn pick(&self, number: usize) {
        // Cloned out of the cell first: what is called takes the menu away.
        let picker = self.picker.borrow().clone();
        if let Some(picker) = picker {
            picker(number);
        }
    }

    /// Sets what is called to draw the menu again, replacing the one before.
    pub fn on_change(&self, redraw: Rc<dyn Fn()>) {
        *self.redraw.borrow_mut() = Some(redraw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::{Edit, ItemKind};

    fn item(label: &str) -> Item {
        Item {
            label: label.to_owned(),
            kind: ItemKind::Function,
            detail: None,
            label_detail: None,
            sort: label.to_owned(),
            filter: label.to_owned(),
            edit: Edit { range: None, text: label.to_owned(), extra: vec![] },
            resolve: None,
            deprecated: false,
        }
    }

    fn many(count: usize) -> Rc<Vec<Item>> {
        Rc::new((0..count).map(|number| item(&format!("item{number:03}"))).collect())
    }

    fn menu_of(labels: &[&str]) -> Menu {
        let list = Rc::new(labels.iter().map(|label| item(label)).collect::<Vec<_>>());
        let view = (0..labels.len() as u32).collect();
        Menu::new(list, view, 4, "").expect("not empty")
    }

    fn shown(menu: &Menu) -> Vec<String> {
        menu.rows().map(|row| row.item.label.clone()).collect()
    }

    #[test]
    fn there_is_no_menu_of_no_items() {
        assert_eq!(Menu::new(many(3), vec![], 0, ""), None);
    }

    #[test]
    fn the_selection_starts_at_the_best_and_wraps_both_ways() {
        let mut menu = menu_of(&["a", "b", "c"]);
        assert_eq!((menu.selected(), menu.chosen().label.as_str(), menu.start(), menu.len()), (0, "a", 4, 3));
        menu.step(false);
        assert_eq!(menu.chosen().label, "c", "up from the first is the last");
        menu.step(true);
        assert_eq!(menu.chosen().label, "a", "down from the last is the first");
        menu.step(true);
        assert_eq!(menu.chosen().label, "b");
        let mut one = menu_of(&["x"]);
        one.step(true);
        one.step(false);
        assert_eq!(one.chosen().label, "x", "a single item stays selected");
    }

    #[test]
    fn only_a_window_of_a_long_list_is_shown_and_it_scrolls_to_keep_the_selection_in_it() {
        let list = many(100);
        let mut menu = Menu::new(Rc::clone(&list), (0..100).collect(), 0, "").unwrap();
        assert_eq!(shown(&menu).len(), WINDOW);
        assert_eq!((shown(&menu)[0].as_str(), shown(&menu)[WINDOW - 1].as_str()), ("item000", "item009"));
        for _ in 0..WINDOW - 1 {
            menu.step(true);
        }
        assert_eq!(shown(&menu)[0], "item000", "the last row of the window is selected, and it has not moved yet");
        menu.step(true);
        assert_eq!((shown(&menu)[0].as_str(), menu.chosen().label.as_str()), ("item001", "item010"), "one more and it moves by one");
        assert!(menu.rows().last().unwrap().selected, "the selected row is the last of the window");
        // Wrapping up from the first goes to the end, where the window is the last rows.
        let mut menu = Menu::new(list, (0..100).collect(), 0, "").unwrap();
        menu.step(false);
        assert_eq!((menu.chosen().label.as_str(), shown(&menu)[0].as_str()), ("item099", "item090"));
        menu.step(true);
        assert_eq!((menu.chosen().label.as_str(), shown(&menu)[0].as_str()), ("item000", "item000"), "and back to the top");
    }

    #[test]
    fn a_page_moves_a_window_and_stops_at_the_ends() {
        let mut menu = Menu::new(many(25), (0..25).collect(), 0, "").unwrap();
        menu.page(true);
        assert_eq!(menu.selected(), WINDOW);
        menu.page(true);
        menu.page(true);
        assert_eq!(menu.selected(), 24, "the last, and not past it");
        assert!(menu.rows().any(|row| row.selected), "and it is shown");
        menu.page(false);
        assert_eq!(menu.selected(), 14);
        menu.page(false);
        menu.page(false);
        assert_eq!((menu.selected(), shown(&menu)[0].as_str()), (0, "item000"), "the first, and not before it");
    }

    #[test]
    fn a_menu_of_fewer_items_than_the_window_shows_them_all() {
        assert_eq!(shown(&menu_of(&["a", "b"])), ["a", "b"]);
    }

    #[test]
    fn a_row_marks_the_part_of_the_label_that_is_the_word() {
        let list = Rc::new(vec![item("push_str"), item("Pushover"), item("x_push"), item("\u{e9}t\u{e9}")]);
        let menu = Menu::new(Rc::clone(&list), vec![0, 1, 2], 0, "push").unwrap();
        assert_eq!(menu.rows().map(|row| row.typed).collect::<Vec<_>>(), [4, 4, 0], "a label that does not begin with it has none to mark");
        // A word that is in the middle of a character of the label does not split it.
        let menu = Menu::new(list, vec![3], 0, "\u{e9}t").unwrap();
        assert_eq!(menu.rows().next().unwrap().typed, 3);
        let short = Menu::new(Rc::new(vec![item("ab")]), vec![0], 0, "abc").unwrap();
        assert_eq!(short.rows().next().unwrap().typed, 0, "a word longer than the label");
    }

    #[test]
    fn the_cell_tells_what_draws_it_only_when_the_menu_changes() {
        let cell = MenuCell::default();
        let drawn = Rc::new(std::cell::Cell::new(0));
        let counted = Rc::clone(&drawn);
        cell.on_change(Rc::new(move || counted.set(counted.get() + 1)));
        assert!(!cell.is_open());
        cell.set(None);
        assert_eq!(drawn.get(), 0, "closed stays closed");
        cell.set(Some(menu_of(&["a", "b"])));
        cell.set(Some(menu_of(&["a", "b"])));
        assert_eq!((drawn.get(), cell.is_open()), (1, true), "the same menu twice is one change");
        cell.step(true);
        assert_eq!(cell.with(|menu| menu.map(|menu| menu.chosen().label.clone())), Some("b".to_owned()));
        assert_eq!(drawn.get(), 2);
        cell.page(false);
        assert_eq!(cell.with(|menu| menu.map(Menu::selected)), Some(0));
        assert_eq!(drawn.get(), 3);
        let taken = cell.take().expect("it was open");
        assert_eq!(taken.chosen().label, "a");
        assert!(!cell.is_open());
        assert_eq!(drawn.get(), 4);
        assert!(cell.take().is_none());
        cell.step(true);
        cell.page(true);
        assert_eq!(drawn.get(), 4, "stepping a closed menu changes nothing");
    }
}
