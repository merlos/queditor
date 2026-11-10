# Queditor website

The marketing site and documentation portal use Astro and Starlight. Repository
Markdown under `../docs/` is canonical; `npm run sync-docs` creates ignored
Starlight copies with frontmatter before each development or production build.

Use Node.js 24 or newer:

```sh
npm ci
npm run dev
npm run build
```

The default local base path is `/`. To reproduce the GitHub project-site build:

```sh
SITE_URL=https://merlos.github.io BASE_PATH=/queditor npm run build
```

`.github/workflows/pages.yml` build-checks pull requests and deploys pushes to
`main` and manual runs. In the repository settings, **Pages → Build and
deployment → Source** must be set to **GitHub Actions**.
