import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// =============================================================================
// GitHub Pages Hosting
// =============================================================================
//
// The docs are meant to work both locally and on GitHub Pages. When the Pages
// workflow provides the final public URL, we split it into the site origin and
// the repository subpath so Astro generates correct canonical URLs and asset
// links for both user sites and project sites.
function resolveSiteConfig() {
  const explicitSite = process.env.DOCS_SITE_URL;
  if (explicitSite) {
    return {
      site: explicitSite,
      base: '/',
    };
  }

  const pagesUrl = process.env.GITHUB_PAGES_URL;
  if (!pagesUrl) {
    return {
      site: 'https://example.com',
      base: '/',
    };
  }

  const resolvedUrl = new URL(pagesUrl);
  const base = resolvedUrl.pathname.replace(/\/$/, '') || '/';

  return {
    site: resolvedUrl.origin,
    base,
  };
}

const { site, base } = resolveSiteConfig();

export default defineConfig({
  site,
  base,
  integrations: [
    starlight({
      title: 'track',
      description:
        'A practical guide to setting up track, configuring remote runs, using the WebUI, and understanding the project as a contributor.',
      customCss: ['./src/styles/custom.css'],
      pagefind: false,
      sidebar: [
        {
          label: 'Initial Setup',
          autogenerate: { directory: 'initial-setup' },
        },
        {
          label: 'Configuring',
          autogenerate: { directory: 'configuring' },
        },
        {
          label: 'Using WebUI',
          autogenerate: { directory: 'using-webui' },
        },
        {
          label: 'Reference',
          autogenerate: { directory: 'reference' },
        },
        {
          label: 'Development Flow',
          badge: { text: 'Developers', variant: 'note' },
          autogenerate: { directory: 'development-flow' },
        },
      ],
    }),
  ],
});
