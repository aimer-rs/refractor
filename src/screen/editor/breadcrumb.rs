use crate::components::icon::{icon, material};
use crate::components::pad::pad;
use crate::document::Document;
use crate::file_icons::file_icon;
use crate::fonts;
use crate::screen::editor::lsp_badge::LspBadge;
use crate::theme::CodeTheme;
use aimer::input::ControllerListener;
use aimer::style::{LayoutSpacing, Theme};
use aimer::{
    BoxAlignment, BuildContext, Container, Expanded, Icons, Key, NumberExt, Row, SizedBox, State,
    StateUpdater, StateView, StatefulWidget, Text, Widget, ZeroSizedBox,
};
use std::path::PathBuf;
use std::rc::Rc;

/// The line under the tabs: where the file is in the folder, and the item of the file the caret is in.
///
/// It follows the document's controller itself, and is drawn again only when the item changes, so
/// typing does not rebuild it. Keyed by the document.
#[derive(Clone, StatefulWidget)]
pub struct Breadcrumb {
    key: Option<Key>,
    document: Rc<Document>,
    /// Where the file is, from the folder.
    path: PathBuf,
}

impl Breadcrumb {
    pub fn new(document: Rc<Document>, path: PathBuf) -> Self {
        Self {
            key: Some(Key::from(("breadcrumb", document.id))),
            document,
            path,
        }
    }

    pub const HEIGHT: i32 = 20;
}

struct BreadcrumbState {
    document: Rc<Document>,
    path: PathBuf,
    /// What the caret is inside of, if it is inside something.
    symbol: Option<String>,
    /// Keeps the symbol following the controller for as long as the line is on screen.
    listener: Option<ControllerListener>,
}

impl BreadcrumbState {
    fn symbol(document: &Document) -> Option<String> {
        let value = document.controller.value();
        document
            .syntax
            .as_ref()
            .and_then(|syntax| syntax.symbol_at(&value.snapshot(), value.selection().focus()))
    }
}

impl State<Breadcrumb> for BreadcrumbState {
    fn create(widget: Breadcrumb) -> Self {
        let symbol = Self::symbol(&widget.document);
        Self {
            document: widget.document,
            path: widget.path,
            symbol,
            listener: None,
        }
    }

    fn on_mount(&mut self, updater: StateUpdater<Self>) {
        let document = Rc::clone(&self.document);
        self.listener = Some(self.document.controller.listen(move |_| {
            let symbol = Self::symbol(&document);
            updater.try_set_state(move |state| {
                if state.symbol != symbol {
                    state.symbol = symbol;
                }
            });
        }));
    }

    fn adopt_config_from(&mut self, new: Self) {
        self.path = new.path;
    }

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let muted = fonts::ui().font_size(12).color(theme.muted);
        let strong = fonts::ui().font_size(12).color(theme.text);
        let separator = || icon(Icons::CHEVRON_RIGHT, 12, theme.muted).boxed();
        let names: Vec<String> = this
            .path
            .iter()
            .map(|part| part.to_string_lossy().into_owned())
            .collect();
        let last = names.len().saturating_sub(1);
        let mut parts = Vec::new();
        for (index, name) in names.into_iter().enumerate() {
            if index > 0 {
                parts.push(separator());
            }
            if index == last {
                parts.push(material(file_icon(&name), 14).boxed());
                parts.push(Text::new(name).text_style(strong).boxed());
            } else {
                parts.push(Text::new(name).text_style(muted).boxed());
            }
        }
        if let Some(symbol) = &this.symbol {
            parts.push(separator());
            parts.push(
                Text::new(symbol.clone())
                    .text_style(strong)
                    .boxed(),
            );
        }

        Container::new()
            .height(Breadcrumb::HEIGHT)
            .padding(pad(4, 14, 4, 14))
            .child(
                Row::new()
                    .vertical_alignment(BoxAlignment::Center)
                    .gaps(LayoutSpacing::all(4))
                    .children(parts),
            )
    }
}
