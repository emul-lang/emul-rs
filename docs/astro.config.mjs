// @ts-check
import fs from 'node:fs';
import path from 'node:path';

import starlight from '@astrojs/starlight';
import { defineConfig } from 'astro/config';
import piton, { compileFile } from 'astro-piton';

import {
  HOME_MODULE,
  REPO_ROOT,
  SPEC_ROOT,
  labelFor,
  parseReference,
  routeFor,
  specModules,
  words,
} from './src/lib/spec.mjs';

/** A TextMate grammar from src/. It gives colours to code blocks. */
const grammar = (/** @type {string} */ file) =>
  JSON.parse(fs.readFileSync(new URL(`./src/${file}`, import.meta.url), 'utf8'));

/**
 * `virtual:emul-spec`: every spec module. This module imports each spec module two
 * times through astro-piton. The JSON gives its structure. The Markdown of the
 * compiler gives its page body. When you add or remove a spec file, the list changes.
 */
function emulSpec() {
  const ID = 'virtual:emul-spec';
  const RESOLVED = '\0' + ID;
  return {
    name: 'emul-spec',
    /** @param {string} id */
    resolveId(id) {
      return id === ID ? RESOLVED : null;
    },
    /** @param {string} id */
    load(id) {
      if (id !== RESOLVED) return null;
      const modules = specModules();
      const lines = modules.flatMap((module, i) => {
        const file = path.join(SPEC_ROOT, `${module}.pi`);
        return [
          `import json${i} from ${JSON.stringify(file)};`,
          `import markdown${i} from ${JSON.stringify(`virtual:piton/markdown/${file}`)};`,
        ];
      });
      const entries = modules.map(
        (module, i) => `  { module: ${JSON.stringify(module)}, json: json${i}, markdown: markdown${i} },`,
      );
      return [...lines, 'export default [', ...entries, '];'].join('\n');
    },
    /** @param {{ watcher: import('vite').FSWatcher, moduleGraph: import('vite').ModuleGraph, ws: { send(payload: { type: 'full-reload' }): void } }} server */
    configureServer(server) {
      const refresh = (/** @type {string} */ file) => {
        if (!file.startsWith(SPEC_ROOT) || !file.endsWith('.pi')) return;
        const module = server.moduleGraph.getModuleById(RESOLVED);
        if (module) server.moduleGraph.invalidateModule(module);
        server.ws.send({ type: 'full-reload' });
      };
      server.watcher.add(SPEC_ROOT);
      server.watcher.on('add', refresh);
      server.watcher.on('unlink', refresh);
    },
  };
}

/** Compiles a spec file to JSON. The compiler runs in the Emul repository. */
function compile(/** @type {string} */ module) {
  return compileFile(path.join(SPEC_ROOT, `${module}.pi`), { cwd: REPO_ROOT });
}

/**
 * The sidebar. It comes from the tables of contents of the specification:
 * `Emul.tableOfContents` in index.pi, the two lists in `Syntax.details`, and
 * the property groups in `Components.properties`. A module that these lists do
 * not include goes under "More".
 */
async function sidebar() {
  const [index, syntax, components] = await Promise.all(['index', 'Syntax', 'Components'].map(compile));
  /** @type {Set<string>} */
  const placed = new Set();
  const item = (/** @type {string} */ from, /** @type {string} */ reference) => {
    const target = parseReference(from, reference);
    if (!target) throw new Error(`emul docs: \`${reference}\` in ${from} is not a reference`);
    placed.add(target.module);
    return { label: labelFor(target.module), link: routeFor(target.module) };
  };
  const references = (/** @type {unknown} */ list) =>
    /** @type {string[]} */ (Array.isArray(list) ? list.filter((r) => typeof r === 'string') : []);

  const introduction = references(index.Emul.tableOfContents).map((r) => item('index', r));
  const [buildingBlocks, rules] = /** @type {unknown[]} */ (syntax.Syntax.details)
    .filter(Array.isArray)
    .map((list) => references(list).map((r) => item('Syntax', r)));

  const properties = Object.entries(components.Components.properties).map(([category, groups]) => ({
    label: words(category[0].toUpperCase() + category.slice(1)),
    collapsed: true,
    items: Object.entries(/** @type {Record<string, unknown>} */ (groups)).flatMap(([group, list]) => {
      const items = references(list).map((r) => item('Components', r));
      // A group of one is the property itself.
      return items.length === 1
        ? items
        : [{ label: words(group[0].toUpperCase() + group.slice(1)), items }];
    }),
  }));

  // Each module with a page that no table of contents lists. A module that
  // compiles to nothing (only abstract anchors) has no page.
  const rest = specModules().filter((m) => !placed.has(m) && m !== HOME_MODULE);
  const compiled = await Promise.all(rest.map(compile));
  const more = rest
    .filter((_, i) => Object.keys(compiled[i]).length > 0)
    .map((m) => ({ label: labelFor(m), link: routeFor(m) }));

  return [
    { label: 'Introduction', items: introduction },
    { label: 'Building blocks', items: buildingBlocks },
    { label: 'Working with Emul', items: rules },
    { label: 'Component properties', items: properties },
    ...(more.length > 0 ? [{ label: 'More', items: more }] : []),
  ];
}

export default defineConfig({
  integrations: [
    piton(),
    starlight({
      title: 'Emul',
      description: 'Emul, a language for designs that render to HTML, SVG and PNG.',
      sidebar: await sidebar(),
      expressiveCode: {
        // `piton.tmLanguage.json` is a copy of the grammar in editors/vscode of Piton.
        shiki: { langs: [grammar('emul.tmLanguage.json'), grammar('piton.tmLanguage.json')] },
      },
    }),
  ],
  vite: {
    plugins: [emulSpec()],
    // The spec is outside the site, in the directory above it.
    server: { fs: { allow: [REPO_ROOT] } },
  },
});
