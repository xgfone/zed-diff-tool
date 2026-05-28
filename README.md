# diff-tool

This is a small Zed translation of `jinsihou.diff-tool-0.0.1`.

Zed extensions cannot read editor buffers directly, so this implementation uses
a minimal language server:

1. Zed sends open buffer contents to the LSP via `textDocument/didOpen` and
   `textDocument/didChange`.
2. The LSP exposes two code actions:
    - `DiffTool: Mark 1st file`
    - `DiffTool: Mark 2nd file`
3. After the second mark, the LSP writes a temporary unified `.diff` file and
   opens it in Zed.

## Development Build

```sh
cargo build --release --manifest-path server/Cargo.toml
cargo build --release --target wasm32-wasip2
```

If `wasm32-wasip2` is not installed:

```sh
rustup target add wasm32-wasip2
```

## Local Development

For local development, this repository includes helper scripts that build the
extension and copy only the extension manifest and wasm into the local Zed data
directory:

```sh
scripts/install-dev.sh
```

For Zed Preview on macOS, pass the data directory explicitly:

```sh
scripts/install-dev.sh --data-dir "$HOME/Library/Application Support/Zed Preview"
```

The script builds these local development artifacts:

- the Zed extension wasm
- the native `diff-tool-lsp` server

It installs only the extension files into:

```text
<Zed data dir>/extensions/installed/diff-tool
```

The local LSP server is not bundled into the extension. Point Zed at the locally
built server with `lsp.diff-tool.binary.path`:

```json
{
    "lsp": {
        "diff-tool": {
            "binary": {
                "path": "/absolute/path/to/diff-tool-lsp"
            }
        }
    }
}
```

Alternatively, set `ZED_DIFF_TOOL_LSP` to the server path before launching Zed.

These scripts are for local development only. Published builds download the
platform-specific LSP server from GitHub Releases or use a user-provided server
path.

Restart Zed after installing or changing the local server configuration.

To remove the local development install:

```sh
scripts/uninstall-dev.sh
```

To remove all local build intermediates and build outputs:

```sh
scripts/clean.sh
```

For development, the extension finds the LSP in this order:

1. Zed `lsp.diff-tool.binary.path` setting
2. `ZED_DIFF_TOOL_LSP`
3. `diff-tool-lsp` on `PATH`
4. A platform-specific binary downloaded from the latest GitHub Release.

For arbitrary projects, the most predictable setup is:

```sh
export ZED_DIFF_TOOL_LSP="$PWD/server/target/release/diff-tool-lsp"
```

Run this command from the `zed-diff-tool` repository root. A bare relative path
such as `server/target/release/diff-tool-lsp` only works when Zed is launched
with `zed-diff-tool` as its working directory, which is not guaranteed.

## Release Packaging

The published extension must not reference local `target/release` paths. Instead,
publish native LSP binaries as GitHub release assets in this form:

```text
diff-tool-lsp-macos-aarch64.zip
diff-tool-lsp-macos-x86_64.zip
diff-tool-lsp-linux-aarch64.zip
diff-tool-lsp-linux-x86_64.zip
diff-tool-lsp-windows-x86_64.zip
```

Each archive should contain a single executable named `diff-tool-lsp`
(`diff-tool-lsp.exe` on Windows). The wasm extension downloads the matching
asset from `xgfone/zed-diff-tool`, caches it under the extension working
directory, and uses that binary for the language server.

The included `.github/workflows/release-lsp.yml` builds and uploads these
assets when a `v*` tag is pushed.

Set `SERVER_REPO` in `src/lib.rs` and `repository` in `extension.toml` to the
actual GitHub repository before publishing if the repository name changes.

## Usage

Open the command palette for the current editor and run code actions. Choose
`DiffTool: Mark 1st file` in the first buffer, then `DiffTool: Mark 2nd file` in
the second buffer. A temporary `.diff` file will open in Zed.

Zed currently does not expose a VSCode-style extension API for adding arbitrary
editor context-menu commands, so this uses LSP code actions instead of right
click menu items.

Zed user configuration also does not provide a way to add custom editor context
menu items or new command palette commands for an extension. Users can, however,
bind Zed's built-in code action picker to a convenient shortcut. Open the keymap
file with `zed: open keymap file` and add a binding such as:

```json
[
    {
        "context": "Editor && mode == full",
        "bindings": {
            "secondary-shift-d": "editor::ToggleCodeActions"
        }
    }
]
```

With the cursor in a supported editor buffer, press that shortcut and choose
`DiffTool: Mark 1st file` or `DiffTool: Mark 2nd file` from the code action
list. The actions are provided by the language server, so they only appear from
Zed's code action UI, not as standalone command palette entries.

After both files are marked, the server writes their current buffer contents to
temporary files and opens Zed's native diff view with the first available Zed
CLI. The server tries both official Zed and Zed Preview command names/paths on
macOS, Linux, and Windows. You can override the opener with `ZED_DIFF_TOOL_ZED`.
While both marked buffers remain open, later edits to either buffer are written
back to the same temporary diff input files, allowing the opened diff view to
refresh from file changes.

## License

The extension code is licensed under the Apache License, Version 2.0. See
[LICENSE](LICENSE).
