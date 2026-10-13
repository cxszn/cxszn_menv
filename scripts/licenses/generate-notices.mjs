import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync, realpathSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const check = process.argv.includes('--check');
const digest = (text) => createHash('sha256').update(text).digest('hex');
const slash = (value) => value.replaceAll('\\', '/');
const compare = (left, right) => left < right ? -1 : left > right ? 1 : 0;
const metadata = JSON.parse(execFileSync('cargo', [
  'metadata', '--format-version', '1', '--locked', '--offline',
  '--filter-platform', 'x86_64-pc-windows-msvc',
], { cwd: root, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 }));
const documents = new Map();
const components = [];

function addDocument(component, name, content) {
  const text = content.replaceAll('\r\n', '\n').replace(/[\t ]+$/gm, '').trimEnd();
  const sha256 = digest(text);
  if (!documents.has(sha256)) documents.set(sha256, text);
  component.documents.push({ file: slash(name), sha256 });
}

function collectDocuments(component, packageRoot, declaredFile) {
  const files = new Set();
  function visit(directory, depth = 0) {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const candidate = path.join(directory, entry.name);
      if (entry.isFile() && /^(licen[cs]e|copying|notice|copyright)([._-]|$)/i.test(entry.name)) {
        files.add(candidate);
      } else if (entry.isDirectory() && depth < 8
        && !['node_modules', '.git', 'target', 'test', 'tests', 'fixtures'].includes(entry.name)
        && !(component.name === 'libsqlite3-sys' && entry.name === 'sqlcipher')) {
        visit(candidate, depth + 1);
      }
    }
  }
  visit(packageRoot);
  if (declaredFile) {
    const declaredPath = path.resolve(packageRoot, declaredFile);
    if (existsSync(declaredPath) && statSync(declaredPath).isFile()) files.add(declaredPath);
  }
  for (const file of [...files].sort(compare)) {
    addDocument(component, path.relative(packageRoot, file), readFileSync(file, 'utf8'));
  }
  component.documents.sort((a, b) => compare(a.file, b.file));
}

for (const pkg of metadata.packages.filter((item) => !metadata.workspace_members.includes(item.id))) {
  const component = {
    ecosystem: 'cargo', name: pkg.name, version: pkg.version,
    declared_license: pkg.license ?? null,
    source: `https://crates.io/api/v1/crates/${encodeURIComponent(pkg.name)}/${encodeURIComponent(pkg.version)}/download`,
    repository: pkg.repository ?? null,
    scope: 'Windows-target resolved Cargo graph (includes build dependencies)',
    documents: [],
  };
  collectDocuments(component, path.dirname(pkg.manifest_path), pkg.license_file);
  component.standalone_notice_found = component.documents.length > 0;
  if (pkg.name === 'selectors' && !component.standalone_notice_found) {
    const content = readFileSync(path.join(path.dirname(pkg.manifest_path), 'lib.rs'), 'utf8');
    const header = content.match(/^\/\*[\s\S]*?\*\//)?.[0];
    if (header) addDocument(component, 'lib.rs (MPL source notice; full license file missing)', header);
  }
  if (pkg.name === 'defmt-parser' && !component.standalone_notice_found) {
    const content = readFileSync(path.join(path.dirname(pkg.manifest_path), 'README.md'), 'utf8');
    const section = content.match(/## License[\s\S]*?(?=\n### Contribution|$)/)?.[0];
    if (section) addDocument(component, 'README.md (License section; referenced full license files missing)', section);
  }
  components.push(component);
}

const sqlite = metadata.packages.find((pkg) => pkg.name === 'libsqlite3-sys');
if (!sqlite) throw new Error('Expected bundled SQLite dependency was not resolved.');
const sqliteRoot = path.dirname(sqlite.manifest_path);
const sqliteSource = readFileSync(path.join(sqliteRoot, 'sqlite3/sqlite3.c'), 'utf8');
const sqliteVersion = sqliteSource.match(/version ([\d.]+)\./)?.[1];
const sqliteNotice = sqliteSource.match(/\/\*\n\*\* 2001 September 15[\s\S]*?\*\//)?.[0];
if (!sqliteVersion || !sqliteNotice) throw new Error('Bundled SQLite source/version notice was not found.');
const sqliteComponent = {
  ecosystem: 'vendored', name: 'SQLite', version: sqliteVersion,
  declared_license: 'Public-domain dedication in source header (not an SPDX inference)',
  source: `https://crates.io/api/v1/crates/libsqlite3-sys/${sqlite.version}/download`,
  repository: 'https://sqlite.org/src',
  scope: `sqlite3/sqlite3.c bundled by libsqlite3-sys ${sqlite.version}; SQLCipher is not enabled`,
  documents: [],
};
addDocument(sqliteComponent, 'sqlite3/sqlite3.c (copyright-disclaimer header)', sqliteNotice);
components.push(sqliteComponent);

const webviewBindings = metadata.packages.find((pkg) => pkg.name === 'webview2-com-sys');
if (webviewBindings) {
  const loader = path.join(path.dirname(webviewBindings.manifest_path), 'x64/WebView2LoaderStatic.lib');
  if (!existsSync(loader)) throw new Error('Expected Windows WebView2 static loader is missing.');
  components.push({
    ecosystem: 'vendored', name: 'Microsoft WebView2 SDK static loader',
    version: `bundled-with-webview2-com-sys-${webviewBindings.version}`,
    declared_license: null,
    source: `https://crates.io/api/v1/crates/webview2-com-sys/${webviewBindings.version}/download`,
    repository: 'https://github.com/wravery/webview2-rs',
    scope: 'x64/WebView2LoaderStatic.lib shipped inside webview2-com-sys; Microsoft SDK terms are not present in the local crate archive and are not inferred from the Rust wrapper MIT metadata.',
    artifact_sha256: digest(readFileSync(loader)), documents: [],
  });
}

const toolchain = execFileSync('rustc', ['--version', '--verbose'], { cwd: root, encoding: 'utf8' });
const toolchainRoot = execFileSync('rustc', ['--print', 'sysroot'], { cwd: root, encoding: 'utf8' }).trim();
const rustRelease = toolchain.match(/^release: (.+)$/m)?.[1].trim();
const rustCommit = toolchain.match(/^commit-hash: (.+)$/m)?.[1].trim();
if (!rustRelease || !rustCommit) throw new Error('Rust toolchain version was not found.');
const rustComponent = {
  ecosystem: 'toolchain', name: 'Rust Standard Library', version: rustRelease,
  declared_license: 'Apache-2.0 OR MIT, with individual-file exceptions in the supplied copyright inventory',
  source: `https://github.com/rust-lang/rust/tree/${rustCommit}/library`,
  repository: 'https://github.com/rust-lang/rust',
  scope: 'Full toolchain-provided standard-library copyright inventory, including conditional platform implementations; this is a conservative superset.',
  documents: [],
};
for (const relative of ['COPYRIGHT-library.html', 'licenses/MIT.txt', 'licenses/Apache-2.0.txt']) {
  const file = path.join(toolchainRoot, 'share/doc/rust', relative);
  if (existsSync(file)) addDocument(rustComponent, `share/doc/rust/${relative}`, readFileSync(file, 'utf8'));
}
components.push(rustComponent);

const visitedNpm = new Set();
function resolveInstalledPackage(name, from) {
  for (let directory = from; ; directory = path.dirname(directory)) {
    const candidate = path.join(directory, 'node_modules', name, 'package.json');
    if (existsSync(candidate)) return realpathSync(path.dirname(candidate));
    if (path.dirname(directory) === directory) return null;
  }
}

function collectNpm(name, from, optional = false) {
  const packageRoot = resolveInstalledPackage(name, from);
  if (!packageRoot) {
    if (optional) return;
    throw new Error(`Production npm dependency is not installed: ${name}`);
  }
  if (visitedNpm.has(packageRoot)) return;
  visitedNpm.add(packageRoot);
  const pkg = JSON.parse(readFileSync(path.join(packageRoot, 'package.json'), 'utf8'));
  const license = typeof pkg.license === 'string' ? pkg.license : pkg.license?.type ?? null;
  const repository = typeof pkg.repository === 'string' ? pkg.repository : pkg.repository?.url ?? null;
  const component = {
    ecosystem: 'npm', name: pkg.name, version: pkg.version, declared_license: license,
    source: `https://registry.npmjs.org/${pkg.name}/-/${pkg.name.split('/').at(-1)}-${pkg.version}.tgz`,
    repository, scope: 'Installed desktop/Tauri production dependency graph (may include compiler tooling)',
    documents: [],
  };
  collectDocuments(component, packageRoot);
  components.push(component);
  for (const dependency of Object.keys(pkg.dependencies ?? {}).sort(compare)) {
    collectNpm(dependency, packageRoot, dependency in (pkg.optionalDependencies ?? {}));
  }
  for (const dependency of Object.keys(pkg.optionalDependencies ?? {}).sort(compare)) {
    collectNpm(dependency, packageRoot, true);
  }
}

for (const relative of ['apps/desktop', 'apps/tauri']) {
  const directory = path.join(root, relative);
  const pkg = JSON.parse(readFileSync(path.join(directory, 'package.json'), 'utf8'));
  for (const name of Object.keys(pkg.dependencies ?? {}).sort(compare)) collectNpm(name, directory);
}

components.sort((a, b) => compare(`${a.ecosystem}/${a.name}/${a.version}`, `${b.ecosystem}/${b.name}/${b.version}`));
const missing = components.filter((item) => !item.documents.length || !item.declared_license || item.standalone_notice_found === false)
  .map(({ ecosystem, name, version, declared_license, documents: found, standalone_notice_found }) => ({
    ecosystem, name, version,
    missing_license_metadata: !declared_license,
    missing_notice_text: !found.length || standalone_notice_found === false,
  }));
const inventory = {
  schema_version: 1,
  target: 'x86_64-pc-windows-msvc',
  generated_by: 'node scripts/licenses/generate-notices.mjs',
  scope: 'Local locked Cargo resolution plus installed desktop/Tauri production npm dependencies; conservative superset, not a binary-link or legal-completeness audit.',
  lock_sha256: {
    'Cargo.lock': digest(readFileSync(path.join(root, 'Cargo.lock'))),
    'pnpm-lock.yaml': digest(readFileSync(path.join(root, 'pnpm-lock.yaml'))),
  },
  exclusions: [
    'MEnv workspace components: no project license has been chosen or granted by this inventory.',
    'Windows and Microsoft Edge WebView2 Runtime are separately supplied platform prerequisites.',
    'PHP runtime archives are downloaded separately and are not bundled in the MEnv executable.',
    'NSIS installer engine and its embedded WebView2 bootstrapper are outside the Cargo/npm source graph; this report is not a complete installer redistribution review.',
  ],
  unresolved: missing,
  components,
};

const parts = [
  '# MEnv third-party notices', '',
  'Generated from locally installed dependency sources by `node scripts/licenses/generate-notices.mjs`.', '',
  'This file preserves available dependency license and notice texts. It does not select or grant a license for MEnv itself and is not a claim of legal completeness. See `scripts/licenses/third-party-inventory.json` for versions, source URLs, scope, document hashes, and unresolved entries.', '',
  'The Cargo graph is filtered for Windows x64 and includes build dependencies. The npm graph follows installed production dependencies of the desktop and Tauri workspaces; some compiler tooling may be included. Listing a component does not assert that its code is linked into the final executable. The exact source archives are linked below, including source availability for components declaring MPL-2.0. Licenses remain as provided by each component; no choice among alternative licenses is made here.', '',
  'Windows and Microsoft Edge WebView2 Runtime are separately supplied platform prerequisites. The executable also uses the Microsoft SDK static loader included in webview2-com-sys; its missing terms are recorded separately from the Rust wrapper license. PHP runtime archives are not bundled. The NSIS installer engine and embedded WebView2 bootstrapper are outside this Cargo/npm inventory and require their own distribution review.', '',
  `Components: ${components.length}. Unique notice texts: ${documents.size}. Unresolved entries: ${missing.length}.`, '',
  '## Unresolved entries', '',
  ...(missing.length ? missing.map((item) => `- ${item.ecosystem}:${item.name}@${item.version}: ${[item.missing_license_metadata && 'license metadata missing', item.missing_notice_text && 'standalone license/notice text missing (any available source-header or README notice is preserved below)'].filter(Boolean).join('; ')}.`) : ['None within the collected dependency graphs.']), '',
  '## Components', '',
];
for (const component of components) {
  parts.push(`### ${component.ecosystem}: ${component.name} ${component.version}`, '',
    `Declared license: ${component.declared_license ?? 'not provided'}.`, '',
    `Source: <${component.source}>`, '',
    ...component.documents.map((document) => `- ${document.file}: [notice text](#notice-${document.sha256})`), '');
}
parts.push('## License and notice texts', '');
for (const [sha256, text] of [...documents.entries()].sort(([a], [b]) => compare(a, b))) {
  const longestFence = Math.max(2, ...[...text.matchAll(/`+/g)].map((match) => match[0].length));
  const fence = '`'.repeat(longestFence + 1);
  parts.push(`<a id="notice-${sha256}"></a>`, '', `### Notice ${sha256}`, '', fence, text, fence, '');
}

for (const [relative, content] of [
  ['THIRD-PARTY-NOTICES.md', `${parts.join('\n').trimEnd()}\n`],
  ['scripts/licenses/third-party-inventory.json', `${JSON.stringify(inventory, null, 2).trimEnd()}\n`],
]) {
  const destination = path.join(root, relative);
  if (check) {
    if (!existsSync(destination) || readFileSync(destination, 'utf8') !== content) {
      throw new Error(`Generated notices are out of date: ${relative}`);
    }
  } else {
    writeFileSync(destination, content, 'utf8');
  }
}
console.log(JSON.stringify({ components: components.length, unique_notice_texts: documents.size, unresolved: missing, check }, null, 2));
