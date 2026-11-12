import { defineCollection } from 'astro:content';
import { docsLoader } from '@astrojs/starlight/loaders';
import { docsSchema } from '@astrojs/starlight/schema';

// The collection of Starlight. This file does not write the documentation pages.
// src/pages/[...slug].astro makes them from the Emul specification.
export const collections = {
  docs: defineCollection({ loader: docsLoader(), schema: docsSchema() }),
};
