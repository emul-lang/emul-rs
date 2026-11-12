# Emul documentation

An [Astro Starlight](https://starlight.astro.build) site generated from the
Emul specification in [`../spec`](../spec). There is no hand-written content:
every page is a spec module, compiled by the Piton compiler through
[`astro-piton`](https://github.com/piton-lang/piton-rs/tree/main/packages/astro-piton)
each time the site builds, and recompiled on save while `npm run dev` runs.

```sh
npm install
npm run dev      # http://localhost:4321
npm run build    # writes dist/
```

`piton` must be on `PATH`.

## How it works

- `astro.config.mjs` registers `astro-piton` and a virtual module,
  `virtual:emul-spec`, which imports every `.pi` file under `spec/` (except
  `index.pi`, `project.pi` and `tests/`) twice: as JSON, for its structure, and as
  `virtual:piton/markdown/...`, the compiler's own Markdown render, for the
  page body.
- `src/pages/[...slug].astro` makes one page per module that exports
  something. Modules holding only abstract anchors compile to nothing and get
  no page. `Thesis` is the home page.
- `src/lib/pages.ts` adapts the compiler's Markdown: references between
  modules become links between pages, headings get ids for the table of
  contents, single newlines in prose become paragraph breaks (the compiler
  writes a paragraph break as one newline), and code blocks go through
  Expressive Code.
- The sidebar is read from the spec's own tables of contents:
  `Emul.tableOfContents` in `index.pi`, the two lists in `Syntax.details`, and
  the groups in `Components.properties`. Any module they leave out is listed
  under "More".
- `src/lib/spec.mjs` maps modules to routes:
  `spec/componentProperties/layout/Display.pi` becomes
  `/component-properties/layout/display/`.
- `src/emul.tmLanguage.json` highlights `emul` code blocks.
  `src/piton.tmLanguage.json` is a copy of the grammar in Piton's
  `editors/vscode`, for `piton` code blocks.

To change the documentation, change the spec.

## Local packages

`astro-piton` and `vite-plugin-piton` are not published to npm, so
`package.json` installs them from a checkout of
[piton-rs](https://github.com/piton-lang/piton-rs) next to this repository's
parent directory (`../../../piton/piton-rs/packages/`). `astro-piton` has to be
built first (`npx -p typescript tsc -p tsconfig.json` in its directory), since
its `dist/` is not committed.

`.npmrc` sets `install-links`, so npm copies the packages rather than linking
them and `astro-piton` can find `vite-plugin-piton`. It also sets
`legacy-peer-deps`, because their peer ranges (Astro 4–5, Vite 5–7) predate
the Astro 7 and Vite 8 this site uses. The plugin works with both. After
rebuilding either package, run `npm install` again to copy the new build.
