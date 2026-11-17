# Graven for Visual Studio Code

Syntax highlighting for the [Graven](https://graven.pages.dev) schema language (`.graven` files) and
for Graven lockfiles (`*.graven.lock`): keywords, declarations (`table`, `struct`, `enum`,
`union`, `service`), field and method IDs, primitive and user types, service methods
(`->`, `Result<T, E>`, `stream`), attributes with their arguments (`#[timeout(ms = 500)]`,
`#[len(min = 1, max = 32), check(slug)]`), defaults and comments.

Navigation across every `.graven` file of the workspace, following unsaved edits:

- **Go to definition** (F12, Ctrl+click) on a type, local or imported (`common.Money`, or an
  alias from `import acme.common as ac;`).
- **Find all references** (Shift+F12) and the highlight of a type's other uses in the file.
- **Hover**: the declaration's kind, qualified name and `///` doc comment.
- **Outline** and breadcrumbs: declarations with their fields, variants and methods.
- **Go to symbol in workspace** (Ctrl+T). Comment toggling, bracket matching and
indentation work as in other languages.

## Install from this repository

```sh
cd editors/vscode
npx @vscode/vsce package          # produces graven-0.1.0.vsix
code --install-extension graven-0.1.0.vsix --force
```

Or, without packaging, link the folder into your extensions directory and restart VS Code:

```sh
ln -s "$PWD" ~/.vscode/extensions/graven
```
