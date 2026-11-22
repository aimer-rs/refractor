use super::completion_caret::CompletionCaret;
use super::context_menu::EditorMenu;
use super::definition::{find_definition, is_the_name_itself};
use super::hover_card::HoverCard;
use super::rename_card::RenameCard;
use super::usages_card::UsagesCard;
use crate::auto_indent::AutoIndent;
use crate::auto_pair::AutoPair;
use crate::completion::Completer;
use crate::components::marker::tinted;
use crate::components::popup::Popup;
use crate::document::{Document, Language};
use crate::fonts;
use crate::hints::{HintColors, Hints};
use crate::hover::{self, HoverCell};
use crate::indent_guides::{Block, GuideStyle, IndentGuides};
use crate::line_comment::LineComment;
use crate::problem_lines::{LineColors, ProblemLines};
use crate::rename::RenamePrompt;
use crate::screen::editor::breadcrumb::Breadcrumb;
use crate::semantic::Semantic;
use crate::settings::{Settings, ThemeMode};
use crate::syntax::{SyntaxHighlighter, SyntaxPalette};
use crate::theme::CodeTheme;
use crate::usages::{LinkColors, Taken, Usages};
use crate::workspace::{Workspace, notifier};
use aimer::events::element::NamedKey;
use aimer::input::input::{
    CaretMark, CaretMarks, GutterMarker, GutterMarkers, LineBar, LineBars, LineLabels,
    LineNumberAlign, LineNumbers, TextHover,
};
use aimer::input::input::{FieldSpot, HoverSpot, TextSnapshot};
use aimer::input::{ControllerListener, TextArea, TextEditingController};
use aimer::style::{LayoutSpacing, TextStyle, Theme};
use aimer::{
    BuildContext, Color, Container, Icons, Key, NumberExt, ProviderContext, ProviderHandle,
    Shortcuts, Stack, State, StateUpdater, StateView, StatefulWidget, Widget,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

/// How wide the column of buttons before the line numbers is.
const MARKER_SIZE: u32 = 10;
/// How wide the bar beside a rendered doc comment is.
const BAR_WIDTH: u32 = 2;

/// One document's code, as a text area. It is keyed by the document, so showing another document
/// makes a text area of its own, and one document keeps its text, caret and undo history in the
/// document for as long as it is open.
#[derive(Clone, StatefulWidget)]
pub struct EditorPane {
    key: Option<Key>,
    document: Rc<Document>,
}

impl EditorPane {
    /// How round the corners of the selection are, in logical pixels: the lines of a selection run into each other.
    const SELECTION_RADIUS: f32 = 4.0;
    /// How long the pointer rests on a name before the card of documentation for it is asked for.
    const HOVER_DELAY: Duration = Duration::from_millis(400);

    pub fn new(document: Rc<Document>) -> Self {
        Self {
            key: Some(Key::from(("document", document.id))),
            document,
        }
    }
}

struct EditorPaneState {
    document: Rc<Document>,
    /// Asks for the pane to be built again, which a button in the margin does after it changes how a
    /// doc comment is shown: the highlighter is made again, and so is what it styled.
    updater: Rc<RefCell<Option<StateUpdater<Self>>>>,
    /// How many times a doc comment was toggled: what makes a build differ from the one before.
    toggles: u32,
    /// Watches the caret, for as long as the pane is on screen: asks for the pane to be built again when
    /// the area has to become read-only or editable, and shows a rendered doc comment that an edit
    /// got into as written.
    listener: Option<ControllerListener>,
    /// Whether the last build made the area read-only, for the listener to compare the caret with.
    guarded: Rc<Cell<bool>>,
    /// The text as one snapshot, shared by everything here that reads it line by line.
    snapshots: Rc<SnapshotCache>,
    /// Offers the keywords of the language as the text is typed. Made with the pane, so each
    /// document has its own menu, and none outlives its pane.
    completer: Rc<Completer>,
    /// Keeps the completer following the controller for as long as the pane is on screen.
    completion_listener: Option<ControllerListener>,
    /// Keeps Shift+Tab the editor's while there is a selection to outdent, for as long as the pane is on screen.
    tab_listener: Option<ControllerListener>,
    /// The card of documentation for the name the pointer rests on. Made with the pane, as the menu is.
    hover: Rc<HoverCell>,
    /// The counts after the definitions, and the list of who uses a name. Made with the pane too.
    usages: Rc<Usages>,
    /// Asks the language server for the counts again when the text has settled, for as long as the pane is on screen.
    usages_listener: Option<ControllerListener>,
    /// The inferred types and the other notes the server has for the lines, drawn in them. Made with the pane too.
    hints: Rc<Hints>,
    /// The colors of names that only the server knows, drawn over the grammar's. Made with the pane too.
    semantic: Rc<Semantic>,
    /// The lines under what the server says is wrong. Made with the pane too.
    problem_lines: Rc<ProblemLines>,
    /// The question that a rename asks, which is over the text while it is open. Made with the pane too.
    rename: Rc<RenamePrompt>,
}

impl State<EditorPane> for EditorPaneState {
    fn create(widget: EditorPane) -> Self {
        Self {
            completer: Completer::new(
                widget.document.language,
                widget.document.controller.clone(),
                widget.document.completions(),
            ),
            document: widget.document,
            updater: Rc::default(),
            toggles: 0,
            listener: None,
            guarded: Rc::default(),
            snapshots: Rc::default(),
            completion_listener: None,
            tab_listener: None,
            hover: Rc::default(),
            usages: Rc::default(),
            usages_listener: None,
            hints: Rc::default(),
            semantic: Rc::default(),
            problem_lines: Rc::default(),
            rename: Rc::default(),
        }
    }

    fn on_mount(&mut self, updater: StateUpdater<Self>) {
        // A rendered doc comment cannot be edited, and only the button in the margin shows it as written.
        // The area is read-only while the caret or the selection is where an edit would change one,
        // which is worked out again when the caret moves, and the pane is built again when that changes.
        if let Some(syntax) = self.document.syntax.clone() {
            let controller = self.document.controller.clone();
            let rebuild = updater.clone();
            let guarded = Rc::clone(&self.guarded);
            let snapshots = Rc::clone(&self.snapshots);
            self.listener = Some(controller.clone().listen(move |_| {
                if guard_of(&syntax, &controller, &snapshots) != guarded.get() {
                    rebuild.try_set_state(|state| state.toggles += 1);
                }
            }));
        }
        self.tab_listener = Some(AutoIndent::hold_tab_while_selected(
            self.document.focus.clone(),
            self.document.controller.clone(),
        ));
        let completer = Rc::clone(&self.completer);
        self.completion_listener = Some(
            self.document
                .controller
                .clone()
                .listen(move |_| completer.refresh()),
        );
        if self.document.language == Language::Rust {
            // The pane is built again when the counts come, as what the area draws is made by the build, and the text is
            // asked about again a moment after it stops changing.
            let rebuild = updater.clone();
            self.usages
                .on_labels_change(Popup::redraw_watcher(rebuild, |state| state.toggles += 1));
            // The same for the notes in the lines: the pane is built again when they come, and they are asked for again as the counts are.
            self.hints
                .on_change(Popup::redraw_watcher(updater.clone(), |state| {
                    state.toggles += 1
                }));
            let hints = Rc::clone(&self.hints);
            // And for the colors of the names: the pane is built again, so that the highlight is made again, when they come.
            self.semantic
                .on_change(Popup::redraw_watcher(updater.clone(), |state| {
                    state.toggles += 1
                }));
            let semantic = Rc::clone(&self.semantic);
            // And for the lines under the problems: the pane is built again, which draws them, when what the server says changes.
            self.problem_lines
                .on_change(Popup::redraw_watcher(updater.clone(), |state| {
                    state.toggles += 1
                }));
            let (usages, watched) = (Rc::clone(&self.usages), self.document.controller.clone());
            let last = RefCell::new(watched.value());
            self.usages_listener = Some(watched.clone().listen(move |_| {
                // Only a change of the text is one: a move of the caret changes what the controller holds too.
                let now = watched.value();
                if now.same_text(&last.borrow()) {
                    return;
                }
                *last.borrow_mut() = now;
                usages.schedule(Usages::EDIT_DELAY);
                hints.schedule(Hints::EDIT_DELAY);
                semantic.schedule(Semantic::EDIT_DELAY);
            }));
            self.usages.schedule(Usages::SHOWN_DELAY);
            self.hints.schedule(Hints::SHOWN_DELAY);
            self.semantic.schedule(Semantic::SHOWN_DELAY);
        } else if self.document.is_cargo_manifest() {
            // The lines under what the schema finds wrong: the pane is built again, which draws them, when they change.
            self.problem_lines
                .on_change(Popup::redraw_watcher(updater.clone(), |state| {
                    state.toggles += 1
                }));
        }
        // Closures made by the first build hold this cell, which is empty until now.
        *self.updater.borrow_mut() = Some(updater);
    }

    fn adopt_config_from(&mut self, _new: Self) {
        // The same document, shown again: nothing here changes.
    }

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let (font_size, tab_spaces, mode) =
            ctx.select::<Settings, (u32, usize, ThemeMode)>(|settings| {
                (
                    settings.font_size(),
                    settings.tab_spaces(),
                    settings.theme(),
                )
            });
        // The colors of the syntax follow the theme: nothing to do when they already do.
        this.document.set_theme(mode);
        if let Some(syntax) = &this.document.syntax {
            syntax.set_code_size(font_size as f32);
            // A long line of a doc comment wraps to a column that reads well, not to the whole window.
            syntax.set_doc_max_width(Some(crate::doc_comments::DocRenderer::READABLE_WIDTH));
        }
        let mut area = TextArea::new()
            // The mark of the language server floats over the top-right of the text: the pointer is an arrow there.
            .pointer_holes(Rc::new(|width, _| super::lsp_badge::covered(width))
                as aimer::input::input::PointerHoles)
            .controller(this.document.controller.clone())
            .focus_node(this.document.focus.clone())
            .expand(true)
            .min_lines(1)
            // Code scrolls sideways; prose wraps.
            .wrap(this.document.language == Language::Markdown)
            .decoration(Color::Transparent)
            .text_style(
                TextStyle::new()
                    .font_family(fonts::code_family())
                    .font_size(font_size)
                    .color(theme.code),
            )
            .selection_color(theme.selection.with_alpha(0.13))
            .selection_radius(EditorPane::SELECTION_RADIUS)
            // The mark that shows where a jump landed is in the color of the theme that marks what is current.
            .locate_color(theme.marker)
            .line_numbers(
                LineNumbers::new()
                    .color(theme.line_number)
                    .active_color(theme.line_number_active)
                    // Numbers are centered in their column, so one and three digit numbers sit on the same axis.
                    .align(LineNumberAlign::Center)
                    .gaps(16, 16),
            )
            .current_line(theme.current_line)
            .tab_spaces(tab_spaces)
            .padding(LayoutSpacing::new().top(8).bottom(8).right(16))
            // The room above the first line and under the last is a margin of the text, which scrolls through it: clipped at the
            // padding it was a dead band of the view, and a rendered block was cut off a few pixels short of the bar below.
            .clip_to_padding(false);
        // The text loses the focus when the user goes to something else: the files may be saved for that, as the settings say.
        {
            let workspace = ProviderHandle::<Workspace>::of(ctx);
            area = area.on_blur(move |_: String| {
                let (auto_save, documents) = {
                    let space = workspace.read();
                    (space.auto_save(), space.documents.clone())
                };
                auto_save.focus_lost(&documents);
            });
        }
        if let Some(syntax) = &this.document.syntax {
            let controller = this.document.controller.clone();
            // The build is what the listener compares the caret with, so it is made from the same
            // answer: read-only where an edit would change a rendered comment.
            let guarded = guard_of(syntax, &controller, &this.snapshots);
            this.guarded.set(guarded);
            area = area.read_only(guarded);
            // Prose is not code: a bracket or a quote typed in markdown is typed as it is, and so is a line made in it.
            let pairs = this.document.language != Language::Markdown;
            let paired = controller.clone();
            let (guard, controller, snapshots) =
                (syntax.clone(), controller, Rc::clone(&this.snapshots));
            let completer = Rc::clone(&this.completer);
            // The list of who uses a name takes the arrows, Return and Escape while it is open, and goes to the place that Return chooses.
            let (list_keys, list_store) = (
                Rc::clone(&this.usages),
                ProviderHandle::<Workspace>::of(ctx),
            );
            let fix_card = Rc::clone(&this.hover);
            area = area.on_key(move |key, modifiers| {
                // With a card open for a problem, Alt+Shift+Return does its first fix and Alt+Return lists the others.
                if modifiers.alt
                    && !modifiers.ctrl
                    && !modifiers.meta
                    && matches!(key, NamedKey::Enter)
                {
                    if modifiers.shift {
                        if let Some(fix) = fix_card.first_fix() {
                            fix_card.hide();
                            fix.run();
                            return true;
                        }
                    } else if fix_card.has_more_fixes() {
                        fix_card.toggle_expanded();
                        return true;
                    }
                }
                if !(modifiers.alt || modifiers.ctrl || modifiers.meta) {
                    match list_keys.take_key(key) {
                        Taken::No => {}
                        Taken::Yes => return true,
                        Taken::Go(place) => {
                            let notify = notifier(&list_store);
                            list_store.update(|space| {
                                let _ = space.go_to_place(&place, notify);
                            });
                            return true;
                        }
                    }
                }
                // An open menu takes the arrows, Return, Tab and Escape before the text does, and nothing leaves
                // the editor with Escape.
                if takes_key(&completer, key) {
                    return true;
                }
                // Alt, Control and Command make Return and Backspace something else: a word or a line is deleted,
                // and Return is submitted. The edits below are for the plain keys, and for text that may be edited.
                let edits =
                    pairs && !guarded && !(modifiers.alt || modifiers.ctrl || modifiers.meta);
                // Tab over a selection puts a level in front of each line it touches, and Shift+Tab takes one from each. With a caret
                // and no selection the key is the field's. A rendered comment is not edited, and prose is indented too.
                if matches!(key, NamedKey::Tab)
                    && !guarded
                    && !(modifiers.alt || modifiers.ctrl || modifiers.meta)
                {
                    return AutoIndent::apply_shift(&controller, tab_spaces, modifiers.shift);
                }
                let backwards = match key {
                    // Return between an opener and its closer puts the closer on a line of its own.
                    NamedKey::Enter => {
                        return edits && AutoIndent::apply_return(&controller, tab_spaces);
                    }
                    NamedKey::Backspace => true,
                    NamedKey::Delete => false,
                    _ => return false,
                };
                // Backspace and Delete at the edge of a comment join the code beside it to the comment,
                // which the caret there does not say.
                if guard.refuses_deletion(
                    &snapshots.of(&controller),
                    controller.value().selection().range(),
                    backwards,
                ) {
                    return true;
                }
                // Backspace between an opener and the closer typed with it takes both, and in a line of only blanks
                // it takes the line.
                edits
                    && backwards
                    && (AutoPair::apply_backspace(&controller)
                        || AutoIndent::apply_backspace(&controller))
            });
            if pairs {
                // A bracket or a quote that is typed closes itself, and is stepped over when it is typed again.
                area = area.on_text_input(move |typed| AutoPair::apply_typed(&paired, typed));
            }
            // What the grammar colors, with the names that the server knows more of drawn over it, once it has said which they are.
            let layer = Rc::clone(&this.semantic);
            area = area.highlighter(
                syntax.highlighter_over(move |runs, text, range| layer.overlay(runs, text, range)),
            );
            // The caret carries the menu, so the menu is wherever the word is being typed.
            let menu = Rc::clone(this.completer.menu());
            area = area
                .caret(move |context| CompletionCaret::new(context, Rc::clone(&menu), theme.code));
            area = area.gutter_markers(doc_markers(&this, theme));

            // Return in a doc comment goes on with `///`: in a rendered block the marker is out of
            // sight, and a plain new line would end the comment without anyone seeing it.
            // Anywhere else a new line is as deep as the one before it, and a level deeper in a block.
            area = area.line_break_prefix(move |before| {
                crate::doc_comments::continuation(before)
                    .or_else(|| pairs.then(|| AutoIndent::continuation(before, tab_spaces)))
            });
            area = area.line_bars(line_bars(&this, mode, font_size, tab_spaces));
            if pairs {
                // The bracket the caret is beside is boxed with the one it matches, in the color of how deep they are, and the
                // two are marked for a moment when the caret comes to them.
                let (marked, palette) = (syntax.clone(), SyntaxPalette::for_mode(mode));
                area = area.caret_marks(CaretMarks::new(move |text, selection| {
                    if !selection.is_empty() {
                        return Vec::new();
                    }
                    marked
                        .pair_beside(text, selection.start)
                        .map_or_else(Vec::new, |pair| {
                            let color = palette.rainbow(pair.depth);
                            vec![
                                CaretMark::new(pair.open, color),
                                CaretMark::new(pair.close, color),
                            ]
                        })
                }));
            }
            // A rendered doc comment is its markdown, as the viewer draws it, in place of its lines.
            area = area.embedder(syntax.doc_embedder());
        }
        if this.document.language == Language::Rust {
            // A force click or a command click on a name goes to where it is defined. The server is asked, and the answer is gone to when it
            // comes, which is not inside this: the workspace is only changed from an update of it.
            let store = ProviderHandle::<Workspace>::of(ctx);
            let path = this.document.path.clone();
            let hover_store = store.clone();
            let force_store = store.clone();
            let go = move |offset: usize| {
                let lsp = store.read().lsp();
                let store = store.clone();
                find_definition(&lsp, &path, offset, move |place| {
                    let notify = notifier(&store);
                    store.update(|space| {
                        let _ = space.go_to_place(&place, notify);
                    });
                });
            };
            // The force click of a trackpad, and Command-click (Control-click off a Mac), do the same: on a name that is defined where it
            // is, they say who uses it, as a click on its count does; on any other name they go to where that is defined.
            let open_link = go.clone();
            let (force_path, force_usages, force_controller) = (
                this.document.path.clone(),
                Rc::clone(&this.usages),
                this.document.controller.clone(),
            );
            let usages_or_definition: Rc<dyn Fn(usize, FieldSpot)> =
                Rc::new(move |offset, spot| {
                    let lsp = force_store.read().lsp();
                    let (store, usages, path, text) = (
                        force_store.clone(),
                        Rc::clone(&force_usages),
                        force_path.clone(),
                        force_controller.snapshot(),
                    );
                    let asked = lsp.clone();
                    find_definition(&lsp, &force_path, offset, move |place| {
                        if let Some(start) =
                            is_the_name_itself(&asked, &text, &path, offset, &place)
                            && usages.show_usages_at(start, spot)
                        {
                            return;
                        }
                        let notify = notifier(&store);
                        store.update(|space| {
                            let _ = space.go_to_place(&place, notify);
                        });
                    });
                });
            let menu_go = Rc::clone(&usages_or_definition);
            let on_command = Rc::clone(&usages_or_definition);
            area = area.on_command_click_at(move |offset, spot| on_command(offset, spot));
            area = area.on_force_click_at(move |offset, spot| usages_or_definition(offset, spot));
            // Only a name is something to jump from: a word of a comment, a string or a raw string is not, and neither
            // is a keyword or a number. The syntax says which, from the parse the highlighting already keeps.
            if let Some(syntax) = &this.document.syntax {
                let (syntax, controller, snapshots) = (
                    syntax.clone(),
                    this.document.controller.clone(),
                    Rc::clone(&this.snapshots),
                );
                let named = syntax.clone();
                let menu_syntax = syntax.clone();
                let (named_controller, named_snapshots) =
                    (controller.clone(), Rc::clone(&snapshots));
                // A click on a link of a rendered doc comment, ``[`Name`]``, goes to what the name is of, as a click on the name
                // written would. The link has the line and the byte of the line, and the text is read when it is clicked.
                let (link_controller, link_snapshots) = (controller.clone(), Rc::clone(&snapshots));
                syntax.set_doc_on_link(move |link| {
                    let text = link_snapshots.of(&link_controller);
                    open_link(
                        crate::doc_comments::line_bytes(&text, link.line).start + link.column,
                    );
                });
                area =
                    area.link_range(move |byte| syntax.link_at(&snapshots.of(&controller), byte));
                // What is written of a name, which the server says, is shown in a card when the pointer rests on one. Only a name
                // that can be jumped from has anything to say: not a comment, a string, a keyword or a number.
                let (ask_store, cancel_store, hover_path) =
                    (hover_store.clone(), hover_store, this.document.path.clone());
                let (dismissed, shut) = (Rc::clone(&this.hover), Rc::clone(&this.usages));
                // What is shown of a place is what is wrong there, with what can be done about it, and the words of the name.
                let source = Rc::new(hover::CardSource::new(
                    Rc::clone(&this.problem_lines),
                    Rc::new(move || ask_store.read().lsp()),
                    hover_path,
                    this.document.controller.clone(),
                ));
                let menu_source = Rc::clone(&source);
                let asking = Rc::clone(&source);
                let follow = hover::follow_with(
                    Rc::clone(&this.hover),
                    move |spot, publish| asking.ask(spot, publish),
                    move || {
                        let _ = &cancel_store;
                        source.cancel();
                    },
                );
                // A problem is what is rested on where there is one, and not the name: the card is for the whole of what is under the line.
                let (problem_lines, line_controller, line_snapshots) = (
                    Rc::clone(&this.problem_lines),
                    this.document.controller.clone(),
                    Rc::clone(&this.snapshots),
                );
                // The card is let stay a moment when the pointer leaves the name, to be got to; a press, a key or a wheel in the text is
                // not on its way anywhere, and takes it at once.
                area = area.on_hover(
                    TextHover::new(EditorPane::HOVER_DELAY, follow)
                        .range_at(move |byte| {
                            if let Some(found) = problem_lines.at(byte).first() {
                                // On the line the pointer is on: a problem that is on many lines has a card at each, and not one as tall
                                // as all of them.
                                let text = line_snapshots.of(&line_controller);
                                let line = text.line_range(text.byte_to_line(byte));
                                let range = found.range.start.max(line.start)
                                    ..found.range.end.min(line.end);
                                if range.start <= byte && byte < range.end {
                                    return Some(range);
                                }
                            }
                            named.link_at(&named_snapshots.of(&named_controller), byte)
                        })
                        .on_dismiss(move || {
                            dismissed.hide();
                            shut.close();
                        }),
                );
                // The right click adds to the clipboard verbs what is about the name it is on: the actions of a problem, the usages, going to where
                // the name is defined, and renaming it.
                let menu = Rc::new(EditorMenu {
                    theme,
                    syntax: menu_syntax,
                    controller: this.document.controller.clone(),
                    problems: Rc::clone(&this.problem_lines),
                    hover: Rc::clone(&this.hover),
                    source: menu_source,
                    usages: Rc::clone(&this.usages),
                    prompt: Rc::clone(&this.rename),
                    go: menu_go,
                });

                area = area
                    .context_menu_items(move |request| menu.extras(request))
                    .context_menu_style(super::context_menu::style(theme));
            }
            // The count of who uses a name, after the line it is defined on, which the server finds and says when it is ready.
            let (root, lsp) = {
                let space = ProviderHandle::<Workspace>::of(ctx);
                let space = space.read();
                (space.root.clone(), space.lsp())
            };
            this.usages.attach(
                lsp,
                this.document.path.clone(),
                root,
                this.document.controller.clone(),
                LinkColors {
                    rest: theme.link,
                    hover: theme.link_hover,
                },
                mode,
                {
                    let buffers = ProviderHandle::<Workspace>::of(ctx);
                    Rc::new(move |path: &std::path::Path| buffers.read().buffer_of(path))
                },
            );
            this.usages.watch_server();
            this.hints.attach(
                store_lsp(ctx),
                this.document.path.clone(),
                this.document.controller.clone(),
                HintColors {
                    text: theme.hint_text,
                    pill: theme.hint_background,
                },
            );
            this.hints.watch_server();
            this.semantic.attach(
                store_lsp(ctx),
                this.document.path.clone(),
                this.document.controller.clone(),
                mode,
            );
            this.semantic.watch_server();
            this.problem_lines.attach(
                store_lsp(ctx),
                this.document.path.clone(),
                this.document.controller.clone(),
                LineColors {
                    error: theme.error_line,
                    warning: theme.warning_line,
                    weak: theme.weak_line,
                },
            );
            // The lines under what is wrong are drawn under the text: a line moves over what is typed, until the server says it again.
            area = area.underlines(this.problem_lines.underlines());
            // The notes of the server are drawn in the lines: what the text is typed in moves them, and the server is asked again.
            area = area.inlays(this.hints.inlays());
            let labels = Rc::clone(&this.usages);
            area = area.line_labels(LineLabels::new(Usages::GAP, move |line| {
                labels.label_of(line)
            }));
        }
        if this.document.is_cargo_manifest() {
            // A manifest has no server: the schema says what is wrong and what each key is, and the lines and the card are made of that.
            this.problem_lines.attach_manifest(
                this.document.path.clone(),
                this.document.controller.clone(),
                LineColors {
                    error: theme.error_line,
                    warning: theme.warning_line,
                    weak: theme.weak_line,
                },
            );
            area = area.underlines(this.problem_lines.underlines());
            let (lines, controller, snapshots) = (
                Rc::clone(&this.problem_lines),
                this.document.controller.clone(),
                Rc::clone(&this.snapshots),
            );
            let ask = {
                let (lines, controller, snapshots) =
                    (Rc::clone(&lines), controller.clone(), Rc::clone(&snapshots));
                move |spot: HoverSpot, publish: Rc<dyn Fn(Option<hover::Card>)>| {
                    let at = spot.range.start;
                    let words = lines
                        .manifest()
                        .hover(&snapshots.of(&controller), at)
                        .map(|hover| hover.markdown)
                        .unwrap_or_default();
                    let problems: Vec<hover::Problem> = lines
                        .at(at)
                        .into_iter()
                        .map(|found| hover::Problem {
                            weight: found.weight,
                            message: found.message,
                            code: found.code,
                        })
                        .collect();
                    if words.is_empty() && problems.is_empty() {
                        publish(None);
                        return;
                    }
                    publish(Some(hover::Card {
                        spot,
                        markdown: words,
                        problems,
                        fixes: Vec::new(),
                    }));
                }
            };
            let follow = hover::follow_with(Rc::clone(&this.hover), ask, || {});
            let dismissed = Rc::clone(&this.hover);
            // A command click or a force click on a path, a member or a feature goes to where it is; the place is underlined as a link
            // while the key is held.
            let manifest_dir = this
                .document
                .path
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_default();
            let jump_at = {
                let (lines, controller, snapshots) =
                    (Rc::clone(&lines), controller.clone(), Rc::clone(&snapshots));
                move |byte: usize| {
                    let scan = lines
                        .manifest()
                        .scan(&snapshots.of(&controller))?;
                    crate::cargo::navigate::jump_at(&scan.entries, byte, &manifest_dir)
                }
            };
            let jump_at = Rc::new(jump_at);
            let (store, path) = (
                ProviderHandle::<Workspace>::of(ctx),
                this.document.path.clone(),
            );
            let go = {
                let jump_at = Rc::clone(&jump_at);
                Rc::new(move |byte: usize, _spot: FieldSpot| {
                    let Some(jump) = jump_at(byte) else {
                        return;
                    };
                    let notify = notifier(&store);
                    store.update(|space| {
                        let _ = match &jump.target {
                            crate::cargo::navigate::Target::File(file) => space.open(file, notify),
                            crate::cargo::navigate::Target::Here(at) => {
                                space.go_to_bytes(&path, *at..*at, notify)
                            }
                        };
                    });
                })
            };
            let on_command = Rc::clone(&go);
            area = area
                .link_range({
                    let jump_at = Rc::clone(&jump_at);
                    move |byte| jump_at(byte).map(|jump| jump.span)
                })
                .on_command_click_at(move |byte, spot| on_command(byte, spot))
                .on_force_click_at(move |byte, spot| go(byte, spot));
            area = area.on_hover(
                TextHover::new(EditorPane::HOVER_DELAY, follow)
                    .range_at(move |byte| {
                        let text = snapshots.of(&controller);
                        if let Some(found) = lines.at(byte).first() {
                            // On the line the pointer is on: a problem on many lines has a card at each.
                            let line = text.line_range(text.byte_to_line(byte));
                            let range =
                                found.range.start.max(line.start)..found.range.end.min(line.end);
                            if range.start <= byte && byte < range.end {
                                return Some(range);
                            }
                        }
                        lines
                            .manifest()
                            .hover(&text, byte)
                            .map(|hover| hover.span)
                    })
                    .on_dismiss(move || dismissed.hide()),
            );
        }
        if this.document.syntax.is_none() {
            // A document that has no syntax has no menu, and its Escape is not the field's either. Tab and Shift+Tab over a selection
            // indent its lines as they do in code.
            let controller = this.document.controller.clone();
            area = area.on_key(move |key, modifiers| match key {
                NamedKey::Escape => true,
                NamedKey::Tab if !(modifiers.alt || modifiers.ctrl || modifiers.meta) => {
                    AutoIndent::apply_shift(&controller, tab_spaces, modifiers.shift)
                }
                _ => false,
            });
        }
        if let Some(markdown) = this.document.markdown() {
            // Rendered wherever the caret is not; the tables and code blocks it is not in are the
            // viewer's widgets.
            area = area
                .highlighter(markdown.highlighter())
                .embedder(markdown.embedder());
        }

        let stack = || {
            Stack::new()
                .add_child(area)
                .add_child(HoverCard::new(Rc::clone(&this.hover), mode))
                .add_child(UsagesCard::new(Rc::clone(&this.usages)))
                .add_child(RenameCard::new(
                    Rc::clone(&this.rename),
                    this.document.path.clone(),
                    this.document.focus.clone(),
                ))
                .boxed()
        };

        // Cmd+/ (Ctrl+/ elsewhere) comments the lines that are selected, or takes the comment away, where the language has one. A
        // rendered doc comment is not edited, as it is not by any other key.
        match this.document.language.line_comment() {
            Some(marker) => {
                let (controller, guarded) =
                    (this.document.controller.clone(), Rc::clone(&this.guarded));
                Shortcuts::new()
                    .bind_str(LineComment::SHORTCUT, move || {
                        if !guarded.get() {
                            LineComment::apply(&controller, marker);
                        }
                    })
                    .box_child(stack())
            }
            None => stack(),
        }
    }
}

/// The language server of the workspace.
fn store_lsp(ctx: &BuildContext) -> crate::lsp::Lsp {
    ProviderHandle::<Workspace>::of(ctx)
        .read()
        .lsp()
}

/// Whether the editor takes `key` before the text does: an open menu takes the arrows, Return, Tab and Escape, and
/// Escape is taken when there is none. Left to the text field, Escape makes it give up the focus, and the caret goes with
/// it: a press meant for a menu that is not there, because the server has not answered yet, is not to cost the caret.
fn takes_key(completer: &Completer, key: &NamedKey) -> bool {
    completer.handle_key(key) || matches!(key, NamedKey::Escape)
}

/// The snapshot of a document's text, made again only when the text or the caret changed. For a short
/// text `controller.snapshot()` builds a new one on every call, which costs the whole text, and the
/// margin asks for one on every visible line of every frame: with this it costs the lines on screen.
#[derive(Default)]
struct SnapshotCache {
    cached: RefCell<Option<(u64, TextSnapshot)>>,
}

impl SnapshotCache {
    /// The snapshot of the text `controller` holds now.
    fn of(&self, controller: &TextEditingController) -> TextSnapshot {
        // A revision is counted for each change of the value, the caret's too, so it never stands for
        // another text than the one it was made for.
        let revision = controller.revision();
        if let Some((cached, snapshot)) = &*self.cached.borrow() {
            if *cached == revision {
                return snapshot.clone();
            }
        }
        let snapshot = controller.snapshot();
        *self.cached.borrow_mut() = Some((revision, snapshot.clone()));
        snapshot
    }
}

/// Whether an edit made now would change a rendered doc comment, which cannot be edited while it is
/// rendered: the caret or the selection is on one, or next to it where a line break would be deleted.
fn guard_of(
    syntax: &SyntaxHighlighter,
    controller: &TextEditingController,
    snapshots: &SnapshotCache,
) -> bool {
    syntax.protects(
        &snapshots.of(controller),
        controller.value().selection().range(),
    )
}

/// The button before the number of the first line of each doc comment: a book open while the comment
/// is rendered, a pen while it is shown as written. Pressing it changes which, and builds the pane
/// again.
fn doc_markers(this: &StateView<EditorPaneState>, theme: CodeTheme) -> Option<GutterMarkers> {
    let syntax = this.document.syntax.clone()?;
    let document = Rc::clone(&this.document);
    let updater = Rc::clone(&this.updater);
    let snapshots = Rc::clone(&this.snapshots);
    Some(GutterMarkers::new(MARKER_SIZE, move |line| {
        let rendered = syntax.doc_block_at(&snapshots.of(&document.controller), line)?;
        let (icon, color) = if rendered {
            (Icons::BOOK_OPEN, theme.marker)
        } else {
            (Icons::PEN_LINE, theme.marker)
        };
        let (syntax, updater) = (syntax.clone(), Rc::clone(&updater));
        Some(GutterMarker::new(
            tinted(icon, color).scene().clone(),
            move || {
                syntax.toggle_doc_block(line);
                if let Some(updater) = updater.borrow().as_ref() {
                    updater.try_set_state(|state| state.toggles += 1);
                }
            },
        ))
    }))
}

/// The bars beside the lines: the one beside a rendered doc comment, down its whole height, at the comment's indent, and the
/// guides down the indents of code, in which the block that holds the caret is bright.
fn line_bars(
    this: &StateView<EditorPaneState>,
    mode: ThemeMode,
    font_size: u32,
    tab_spaces: usize,
) -> Option<LineBars> {
    let syntax = this.document.syntax.clone()?;
    let document = Rc::clone(&this.document);
    let snapshots = Rc::clone(&this.snapshots);
    let doc_color = crate::markdown::doc_color(mode).with_alpha(0.7);
    // Prose has no indents to guide.
    let guided = this.document.language != Language::Markdown;
    let style = GuideStyle {
        palette: SyntaxPalette::for_mode(mode),
        unit: tab_spaces.max(1),
        code_size: font_size as f32,
    };
    // The block the caret is in, found again only when the text or the caret is not what it was found for.
    let held: RefCell<Option<(u64, Option<Block>)>> = RefCell::new(None);
    Some(LineBars::many(BAR_WIDTH, move |line, context, bars| {
        let text = snapshots.of(&document.controller);
        if guided {
            // A field that has no focus has no caret, and so no block that holds it.
            let block = context.selection().and_then(|_| {
                let revision = document.controller.revision();
                let mut held = held.borrow_mut();
                if held
                    .as_ref()
                    .is_none_or(|(known, _)| *known != revision)
                {
                    let caret = document
                        .controller
                        .value()
                        .selection()
                        .range()
                        .end;
                    let pair = syntax
                        .pair_beside(&text, caret)
                        .or_else(|| syntax.pair_around(&text, caret));
                    let block = pair.and_then(|pair| {
                        IndentGuides::block(&text, &pair.open, &pair.close, style.unit)
                    });
                    *held = Some((revision, block));
                }
                held.as_ref()
                    .and_then(|(_, block)| block.clone())
            });
            IndentGuides::bars(&text, line, block.as_ref(), &style, bars);
        }
        if let Some(indent) = syntax.doc_bar(&text, line) {
            bars.push(LineBar::new(
                doc_color,
                crate::doc_comments::indent_px(indent, font_size as f32).round() as u32,
            ));
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_is_never_the_fields_so_that_the_editor_does_not_lose_its_caret() {
        let controller = TextEditingController::with_text("");
        let completer = Completer::new(Language::Rust, controller.clone(), None);
        let listener = {
            let completer = Rc::clone(&completer);
            controller
                .clone()
                .listen(move |_| completer.refresh())
        };
        // Without a menu, Escape is taken all the same: left to the field it would give up the focus, and the caret with it.
        assert!(takes_key(&completer, &NamedKey::Escape), "no menu");
        // With one, it closes it, and that is the whole of what it does.
        controller.replace_range(0..0, "pu");
        assert!(completer.menu().is_open());
        assert!(takes_key(&completer, &NamedKey::Escape));
        assert!(!completer.menu().is_open());
        assert!(takes_key(&completer, &NamedKey::Escape), "and again");
        // Other keys are the field's when there is no menu.
        assert!(!takes_key(&completer, &NamedKey::ArrowLeft));
        assert!(!takes_key(&completer, &NamedKey::Backspace));
        drop(listener);
    }

    #[test]
    fn the_snapshot_is_made_once_for_a_text_and_again_for_an_edit() {
        let controller = TextEditingController::with_text("/// doc\nfn x() {}\n");
        let cache = SnapshotCache::default();
        let first = cache.of(&controller);
        assert!(
            first.ptr_eq(&cache.of(&controller)),
            "the same text is the same snapshot: this is what the margin asks for on each line"
        );
        controller.select(3, 3);
        assert_eq!(
            cache.of(&controller).slice(0..7),
            "/// doc",
            "a move of the caret leaves the text as it was"
        );
        controller.set_text("/// docs\nfn x() {}\n");
        let edited = cache.of(&controller);
        assert!(!first.ptr_eq(&edited), "an edit makes a new one");
        assert_eq!(edited.slice(0..8), "/// docs");
        assert!(edited.ptr_eq(&cache.of(&controller)));
    }
}
