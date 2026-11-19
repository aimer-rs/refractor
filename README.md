# Refractor

A code editor built with the [Aimer](https://github.com/aimer-rs/aimer) framework.

## What it does

- Shows the folder as a tree of files and opens them in tabs. The tree is flat, like most editors', with a
  Material Icon Theme icon for each kind of file and folder (`assets/icons`), and its width is dragged
  by its right edge. Git's view of each file is shown as a color and a letter (`M`, `U`, `A`, `D`, `R`), a
  dot on a folder with changes inside, and quiet text for what `.gitignore` names. It is asked of the `git`
  program off the UI thread, when the folder opens and after a save.
- Edits a file in JetBrains Mono (`assets/JetBrainsMono-2.304`, embedded) with line numbers, a band behind the current line, no line wrapping (a long line scrolls
  sideways) and the keys of a text editor.
- Saves with `Cmd+S` (`Ctrl+S` elsewhere). The file is replaced whole or not at all, keeps its permissions, and
  is written through a link to the file the link points to. A file with unsaved changes has a dot on its tab, and
  closing its tab asks twice.
- Renders markdown: a `.md` file shows rendered wherever the caret is not (the line the caret is on shows its
  source). The doc comments (`///`, `//!`) of Rust are drawn by the framework's markdown viewer, in the UI font
  in a muted green, with one bar down the whole comment: a rendered comment stands in for all its lines, so it has
  one line number and the height of its text. A button before the line number (a book while it is rendered, a pen
  while it is shown as written) switches the comment; so does a click on it, and typing into it. Return in a doc
  comment goes on with `///`. See `src/markdown.rs` and `src/doc_comments.rs`.
- Colors Rust by its syntax. The parse is kept between keystrokes and made again from the previous tree, so
  an edit costs the part of the tree it changed, and only the lines on screen are styled.
- Shows where the file is under the tabs: the path from the folder and the item the caret is inside of
  (`mod app_shell`, `impl Point`, `fn norm`). The status bar has the branch (`main*` when something is
  changed) and the caret's line and column, the indent, the encoding, the line endings and the language.
- Starts on the folder named on the command line, `REFRACTOR_ROOT`, the folder it was started in, or the
  folder it was built from. A folder dropped on it becomes the folder, and a source file opens in a tab.

## Layout

| Path | What is in it |
| --- | --- |
| `src/doc_comments.rs`, `src/markdown.rs` | Markdown in doc comments and `.md` files: finding the blocks, rendering them, and the theme |
| `src/syntax.rs`, `src/syntax/` | The incremental tree-sitter highlighter, its palette and the symbol under the caret |
| `src/document.rs` | A file that is open: its text, caret, syntax, whether it changed, and saving it |
| `src/workspace.rs` | The folder, the open files and which is shown |
| `src/tree.rs`, `src/git.rs` | Reading a folder into rows, and the branch and status git reports |
| `src/file_icons.rs`, `src/fonts.rs` | Which icon a file gets, and the fonts (Google Sans for the window, JetBrains Mono for code) |
| `src/screen/editor/` | The window: file tree, tabs, breadcrumb, code view and status bar |
| `src/app_shell/` | The theme and the opening of files |

## Working on it

It depends on the framework at `../aimer` by path, and some of what it uses (the no-wrap editor surface, the
line-number margin, the controller's listener) lives there. See `AGENTS.md`.

```bash
CARGO_TARGET_DIR="agents/target" cargo nextest run
```

## Not done yet

Missing: search, a git panel, the other buttons of the tree's header (new file, new folder), choosing a folder from a dialog (the framework has no folder picker; drop a folder on the app),
languages other than Rust, a light theme, creating, renaming and deleting files, watching the disk for
changes made elsewhere, find and replace, and an editor for files too large to parse once (a file past 4 MB
is shown without colors).
