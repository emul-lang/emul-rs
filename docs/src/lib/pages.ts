// Turns the compiled specification into pages.
//
// Each spec module comes two times. The Piton compiler compiles both through
// astro-piton. The JSON gives the structure. The Markdown of the compiler gives the
// page body. This file does not render the spec again. It adapts the Markdown of
// the compiler to the site. Links between modules become site routes. Headings
// get ids for the table of contents. Code blocks go through the Expressive Code
// of Starlight.

import GithubSlugger from 'github-slugger';
import { Marked, type Token, type Tokens } from 'marked';

import { labelFor, linkFor, slugFor, words } from './spec.mjs';

/** One spec module, as `virtual:emul-spec` provides it. */
export interface SpecModule {
  /** The path below the spec root without `.pi`, such as `Values`. */
  module: string;
  /** The compiled module: one key for each export. */
  json: Record<string, unknown>;
  /** The module as the compiler renders it to Markdown. */
  markdown: string;
}

export type Block = { type: 'html'; html: string } | { type: 'code'; code: string; lang: string };

export interface Heading {
  depth: number;
  slug: string;
  text: string;
}

export interface Page {
  module: string;
  slug: string | undefined;
  title: string;
  description: string | undefined;
  headings: Heading[];
  blocks: Block[];
}

const FENCE = /^\s*(```|~~~)/;

/** A line of prose. The compiler puts one newline between it and the next line. */
function isProse(line: string): boolean {
  return line.trim() !== '' && !/^(#|[-*+] |\d+\. |>|\||\s|```|~~~)/.test(line);
}

/**
 * Adapts the Markdown of the compiler to a page. `lift` removes the one top-level
 * heading, because that heading is the page title. Else, each heading moves one
 * level lower. Then each export gets its own section.
 */
function adapt(module: string, markdown: string, lift: boolean): string {
  const lines = markdown.replace(/\s+$/, '').split('\n');
  const out: string[] = [];
  let fenced = false;
  for (let i = 0; i < lines.length; i++) {
    let line = lines[i];
    if (FENCE.test(line)) {
      fenced = !fenced;
      out.push(line);
      continue;
    }
    if (fenced) {
      out.push(line);
      continue;
    }
    if (/^#{1,6} /.test(line)) {
      if (lift && line.startsWith('# ')) {
        continue;
      }
      if (!lift) {
        line = '#' + line;
      }
    }
    // Links between spec modules become links between pages.
    line = line.replace(/\]\(([^)\s]+)\)/g, (whole, href: string) => {
      const link = linkFor(module, href);
      return link ? `](${link})` : whole;
    });
    out.push(line);
    // The compiler writes a paragraph break in prose as one newline. Markdown reads
    // one newline as part of the same paragraph.
    if (isProse(line) && i + 1 < lines.length && isProse(lines[i + 1])) {
      out.push('');
    }
  }
  return out.join('\n').replace(/^\n+/, '');
}

/** Splits Markdown into HTML and code blocks. It also collects the headings for the
 * table of contents. */
function toBlocks(markdown: string): { blocks: Block[]; headings: Heading[] } {
  const slugger = new GithubSlugger();
  const headings: Heading[] = [];
  const marked = new Marked({
    gfm: true,
    renderer: {
      heading(this: { parser: { parseInline(t: Token[]): string } }, { tokens, depth, text }: Tokens.Heading) {
        const slug = slugger.slug(text);
        headings.push({ depth, slug, text });
        return `<h${depth} id="${slug}">${this.parser.parseInline(tokens)}</h${depth}>\n`;
      },
    },
  });
  const tokens = marked.lexer(markdown);
  const blocks: Block[] = [];
  let pending: Token[] = [];
  const flush = () => {
    if (pending.length === 0) return;
    const list = Object.assign(pending, { links: tokens.links });
    blocks.push({ type: 'html', html: marked.parser(list) });
    pending = [];
  };
  for (const token of tokens) {
    if (token.type === 'code') {
      flush();
      const code = token as Tokens.Code;
      blocks.push({ type: 'code', code: code.text, lang: code.lang?.trim() || 'text' });
    } else {
      pending.push(token);
    }
  }
  flush();
  return { blocks, headings };
}

/** The first sentence of the description of an anchor, for the meta description of
 * the page. */
function describe(value: unknown): string | undefined {
  const anchor = value as { description?: unknown } | undefined;
  if (typeof anchor?.description !== 'string') return undefined;
  const first = anchor.description.split('\n')[0].trim();
  return first.match(/^.*?[.!?](?=\s|$)/)?.[0] ?? first;
}

/** Builds a page for every spec module that exports something to document. */
export function buildPages(modules: SpecModule[]): Page[] {
  const pages: Page[] = [];
  for (const { module, json, markdown } of modules) {
    const exports = Object.keys(json);
    // A module that has only abstract anchors compiles to nothing, for example a
    // `lib/` of shared shapes. Thus it has no page.
    if (exports.length === 0) continue;
    const single = exports.length === 1;
    const { blocks, headings } = toBlocks(adapt(module, markdown, single));
    const name = module.split('/').pop()!;
    pages.push({
      module,
      slug: slugFor(module),
      title: single ? labelFor(module) : words(name),
      description: single ? describe(json[exports[0]]) : undefined,
      headings,
      blocks,
    });
  }
  return pages;
}
