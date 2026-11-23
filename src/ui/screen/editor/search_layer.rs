use super::search_popup::SearchPopup;
use crate::services::search::Search;
use crate::model::workspace::Workspace;
use aimer::shortcut::ChordMods;
use aimer::{
    AnyWidget, BuildContext, ChildBuilder, ProviderHandle, Shortcuts, Stack, State, StateView,
    StatefulWidget, Widget, ZeroSizedBox,
};
use std::rc::Rc;

/// Puts the search over its child: pressing `Shift` twice opens it, and what it needs is made then and not before, so a window
/// that never searches pays nothing for it. While it is open the `Shift` that is held shows the preview of the hit.
///
/// The child is always the first of the layers, so that opening the search does not make it again.
#[derive(Clone, StatefulWidget)]
pub struct SearchLayer {
    child: ChildBuilder,
}

impl SearchLayer {
    pub fn new(child: impl Widget + 'static) -> Self {
        Self {
            child: ChildBuilder::from_widget(child),
        }
    }
}

struct SearchLayerState {
    child: ChildBuilder,
    /// The search that is open, if one is.
    search: Option<Rc<Search>>,
}

impl State<SearchLayer> for SearchLayerState {
    fn create(widget: SearchLayer) -> Self {
        Self {
            child: widget.child,
            search: None,
        }
    }

    fn adopt_config_from(&mut self, new: Self) {
        self.child = new.child;
    }

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let store = ProviderHandle::<Workspace>::of(ctx);
        let updater = this.updater();
        let open = move || {
            // Read here and not in the update: what the search needs of the workspace is a folder and the server.
            let (root, lsp) = {
                let space = store.read();
                (space.root.clone(), space.lsp())
            };
            updater.set_state(move |state| {
                // A second double tap while it is open is the same search, which keeps what was typed.
                if state.search.is_none() {
                    state.search = Some(Search::new(root, lsp));
                }
            });
        };
        let updater = this.updater();
        let popup: AnyWidget = match &this.search {
            Some(search) => SearchPopup::new(
                Rc::clone(search),
                Rc::new(move || updater.set_state(|state| state.search = None)),
            )
            .boxed(),
            None => ZeroSizedBox.boxed(),
        };
        let watched = this.search.clone();
        Shortcuts::new()
            .window(true)
            .bind_str("DoubleShift", open)
            .on_modifiers(move |held| {
                if let Some(search) = &watched {
                    search.set_shift(held.contains(ChordMods::SHIFT));
                }
            })
            .child(
                Stack::new()
                    .add_child(AnyWidget::from(this.child.clone().boxed()))
                    .add_child(popup),
            )
    }
}
