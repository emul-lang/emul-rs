# Emul

Emul is a small design language for describing user interfaces. It borrows the
layout and styling model of HTML and CSS, without the legacy quirks. This crate
is its compiler, `emul`, which renders compositions to HTML, SVG and PNG.

```emul
component Button:
    htmlTag: button
    padding: 8px 12px
    borderRadius: 4px
    backgroundColor: #e4e4e7

component SecondaryButton extends Button:
    backgroundColor: #ffffff
    border: 1px solid #dc2626

export composition Buttons {
    Button { "Save" }
    SecondaryButton { "Cancel", title: Discard changes }
}
```

The language is specified in [`spec/`](spec), which is written in Piton.
Start at [`spec/index.pi`](spec/index.pi).

## Install

You need a Rust toolchain that supports edition 2024 (Rust 1.85 or later).

```sh
cargo xtask install
```

This builds a release binary and installs it with `cargo install`, so it ends
up in `~/.cargo/bin`. Anything after `install` is passed on to `cargo install`,
for example `cargo xtask install --root ~/.local`.

## Usage

Run `emul` from a project directory. It reads `emul.config.pi` from the current
directory if there is one, and otherwise renders `./index.emul` with the
defaults.

| Command          | What it does                                                    |
| ---------------- | --------------------------------------------------------------- |
| `emul build`     | Check the project, then render every exported composition       |
| `emul check`     | Report errors and warnings without rendering                    |
| `emul format`    | Rewrite `.emul` files in their canonical layout (`--check` to verify) |
| `emul preview`   | Open a live viewer that refreshes whenever a file is saved      |

`build` and `preview` accept `--width`, `--height`, `--scale`, `--background`,
`--format html|svg|png`, `--svg-fonts outline|embed`, `--out` and `--config`.
Run `emul <command> --help` for details.

### Configuration

```pi
use @emul/config

export emul-config Project:
    entry: ./index.emul
    outDir: ./dist
    width: 900
    background: #fafafa
    sizes:
        Features:
            height: 700
```

See [`spec/Config.pi`](spec/Config.pi) for every setting.

## Samples

```
samples/
  lib/        shared components, one per file; index.emul re-exports them all
  cards/      a card grid and some buttons
  features/   a tour of grid areas, borders, gradients, clamping and stacking
```

The samples import what they need from the shared library:

```emul
from ../lib import Card, Title, Copy
```

To render one, run `emul build` from inside its directory:

```sh
cd samples/cards && emul build      # writes out/Cards.{html,svg,png}
```

## Development

```sh
cargo build
cargo run -- build --config samples/features/emul.config.pi
```

## License

[MIT](LICENSE) © 2026 Derek McTavish Mounce
