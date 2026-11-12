// The location of the Emul specification, and how its modules and references
// map to pages on this site. astro.config.mjs (sidebar, virtual module) and the
// page route both use this file. Thus a module, a reference and a link agree.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

/** The Emul repository: the directory that holds piton.config.pi. */
export const REPO_ROOT = path.resolve(fileURLToPath(new URL('.', import.meta.url)), '../../..');
/** The source root of the specification, from piton.config.pi. */
export const SPEC_ROOT = path.join(REPO_ROOT, 'spec');

/** The module that gives the home page of the site. */
export const HOME_MODULE = 'Thesis';

/**
 * Every spec module that can give a page. Each is a path below the spec root,
 * without `.pi`, for example `componentProperties/layout/Display`.
 *
 * This list does not include `index.pi` or `project.pi`. `index.pi` is the
 * entry of the `emul` package, and the config reads its table of contents.
 * `project.pi` exports the crux skills again, and it compiles only with the
 * project config. The list also does not include `tests/`. That directory holds
 * `piton test` statements, not documentation.
 */
export function specModules(dir = SPEC_ROOT) {
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    const rel = path.relative(SPEC_ROOT, full);
    if (entry.isDirectory()) {
      if (rel !== 'tests') out.push(...specModules(full));
    } else if (entry.name.endsWith('.pi') && rel !== 'index.pi' && rel !== 'project.pi') {
      out.push(rel.slice(0, -'.pi'.length).split(path.sep).join('/'));
    }
  }
  return out.sort();
}

/** `componentProperties` → `component-properties`, `HtmlOutput` → `html-output`. */
export function kebab(name) {
  return name
    .replace(/([a-z0-9])([A-Z])/g, '$1-$2')
    .replace(/([A-Z]+)([A-Z][a-z])/g, '$1-$2')
    .toLowerCase();
}

/** `HtmlOutput` → `Html Output`, `BoxShadow` → `Box Shadow`. */
export function words(name) {
  return name.replace(/([a-z0-9])([A-Z])/g, '$1 $2').replace(/([A-Z]+)([A-Z][a-z])/g, '$1 $2');
}

/** The URL path of the page of a module: `/component-properties/layout/display/`. */
export function routeFor(module) {
  if (module === HOME_MODULE) return '/';
  return '/' + module.split('/').map(kebab).join('/') + '/';
}

/** The route parameter for the page of a module, for `[...slug].astro`. */
export function slugFor(module) {
  return module === HOME_MODULE ? undefined : module.split('/').map(kebab).join('/');
}

/** Resolves `target` to a module. `target` is a path relative to the directory of
 * `from`, with no extension. */
function resolveModule(from, target) {
  return path.posix.normalize(path.posix.join(path.posix.dirname(from), target));
}

/**
 * Reads a compiled JSON reference in module `from`, for example
 * `./Values.json:Values` or `./Button.json:Button.color`.
 */
export function parseReference(from, reference) {
  const match = /^(.*)\.json:([^.]+)(?:\.(.+))?$/.exec(reference);
  if (!match) return null;
  return { module: resolveModule(from, match[1]), anchor: match[2], property: match[3] ?? null };
}

/**
 * The site link for a compiled Markdown link in module `from`, for example
 * `./componentProperties/layout/Display.md#display`. This function removes a
 * fragment that names the anchor itself, because the anchor is the page.
 * Returns null for a link that does not go to a spec module.
 */
export function linkFor(from, href) {
  const match = /^(\.{1,2}\/.*)\.md(?:#(.*))?$/.exec(href);
  if (!match) return null;
  const module = resolveModule(from, match[1]);
  const fragment = match[2] ?? '';
  const anchor = module.split('/').pop();
  const keep = fragment && fragment !== kebab(anchor) && fragment !== anchor.toLowerCase();
  return routeFor(module) + (keep ? `#${fragment}` : '');
}

/** The sidebar label for a module: the identifier of a property, or else the name in words. */
export function labelFor(module) {
  const name = module.split('/').pop();
  if (module.startsWith('componentProperties/')) {
    return name[0].toLowerCase() + name.slice(1);
  }
  return words(name);
}
