export const REPO_ROOT: string;
export const SPEC_ROOT: string;
export const HOME_MODULE: string;
export function specModules(dir?: string): string[];
export function kebab(name: string): string;
export function words(name: string): string;
export function routeFor(module: string): string;
export function slugFor(module: string): string | undefined;
export function parseReference(
  from: string,
  reference: string,
): { module: string; anchor: string; property: string | null } | null;
export function linkFor(from: string, href: string): string | null;
export function labelFor(module: string): string;
