import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const website = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repository = resolve(website, '..');
const destination = join(website, 'src', 'content', 'docs', 'docs');

const documents = [
  ['user-guide.md', 'User guide', 'Learn Queditor’s editing, file, chat, and review workflows.'],
  ['configuration.md', 'Configuration', 'Configure themes, keybindings, models, and custom agents.'],
  ['live-ai.md', 'Live AI, privacy, permissions, and recovery', 'Connect providers while keeping context sharing and file access explicit.'],
  ['releasing.md', 'Releasing Queditor', 'Build and publish Queditor’s unsigned release archives.'],
];

await mkdir(destination, { recursive: true });

for (const [filename, title, description] of documents) {
  const source = join(repository, 'docs', filename);
  const markdown = await readFile(source, 'utf8');
  const withoutHeading = markdown.replace(/^# .+\r?\n+/, '');
  const frontmatter = `---\ntitle: ${JSON.stringify(title)}\ndescription: ${JSON.stringify(description)}\n---\n\n`;
  await writeFile(join(destination, filename), frontmatter + withoutHeading, 'utf8');
}
