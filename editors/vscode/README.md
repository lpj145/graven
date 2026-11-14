# Graven for Visual Studio Code

Syntax highlighting for the [Graven](../../README.md) schema language (`.graven` files) and
for Graven lockfiles (`*.graven.lock`): keywords, declarations, field IDs, primitive and
user types, attributes, defaults and comments. Comment toggling, bracket matching and
indentation work as in other languages.

## Install from this repository

```sh
cd editors/vscode
npx @vscode/vsce package          # produces graven-0.1.0.vsix
code --install-extension graven-0.1.0.vsix
```

Or, without packaging, link the folder into your extensions directory and restart VS Code:

```sh
ln -s "$PWD" ~/.vscode/extensions/graven
```
