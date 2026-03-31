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
function splitPublicUrl(publicUrl) {
  const resolvedUrl = new URL(publicUrl);
  const base = resolvedUrl.pathname.replace(/\/$/, '') || '/';

  return {
    site: resolvedUrl.origin,
    base,
  };
}

function resolveSiteConfig() {
  const explicitSite = process.env.DOCS_SITE_URL;
  if (explicitSite) {
    return splitPublicUrl(explicitSite);
  }

  const pagesUrl = process.env.GITHUB_PAGES_URL;
  if (pagesUrl) {
    return splitPublicUrl(pagesUrl);
  }

  // Local development is easier to work with at the root path, but production
  // builds should default to the published GitHub Pages URL for this repo.
  if (process.env.NODE_ENV !== 'production') {
    return {
      site: 'http://localhost:4321',
      base: '/',
    };
  }

  return {
    site: 'https://popzxc.github.io',
    base: '/track',
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
        'Set up track, capture work from the CLI, run remote sessions, and manage reviews from the WebUI.',
      customCss: ['./src/styles/custom.css'],
      pagefind: true,
      social: [
        {
          icon: 'github',
          label: 'GitHub',
          href: 'https://github.com/popzxc/track',
        },
      ],
      components: {
        ThemeProvider: './src/components/StaticDarkThemeProvider.astro',
        ThemeSelect: './src/components/NoThemeSelect.astro',
      },
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
          autogenerate: { directory: 'development-flow' },
        },
      ],
    }),
  ],
});
