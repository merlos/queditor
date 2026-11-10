import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

const repository = process.env.GITHUB_REPOSITORY?.split('/')[1];
const base = process.env.BASE_PATH ?? (repository ? `/${repository}` : '/');

export default defineConfig({
  site: process.env.SITE_URL ?? 'https://merlos.github.io',
  base,
  integrations: [
    starlight({
      title: 'Queditor',
      description: 'A terminal editor where every AI edit waits for your review.',
      logo: { src: './src/assets/logo.svg' },
      favicon: '/favicon.svg',
      social: [
        { icon: 'github', label: 'GitHub', href: 'https://github.com/merlos/queditor' },
      ],
      customCss: ['./src/styles/starlight.css'],
      sidebar: [
        {
          label: 'Documentation',
          items: [
            { label: 'Overview', slug: 'docs' },
            { label: 'User guide', slug: 'docs/user-guide' },
            { label: 'Configuration', slug: 'docs/configuration' },
            { label: 'Live AI & privacy', slug: 'docs/live-ai' },
            { label: 'Releasing', slug: 'docs/releasing' },
          ],
        },
      ],
    }),
  ],
});
