// stage.mjs - writes web/, the static site of the Pharo VM for WebAssembly
//
//   node stage.mjs --out <web dir> --module <pharo-web.js> --stock-image <dir>
//        --web-package <OSWebDriver.class.st>
//        [--web <dir>] [--memory64 1|2] [--git <sha>] [--ffi 0|1]
//        [--fonts freetype|bitmap] [--libgit2 0|1] [--libgit2-http 0|1]
//        [--notices <file>] [--st <file>]... [--library-file <name>]...
//        [--world-image <dir> [--world-st <file>]...]
//        [--image-id <id>] [--extra-image <id>=<dir>]...
//        [--extra-world-image <id>=<dir>]... [--extra-sources <file>]...
//        [--git-proxy-default <url>]
//   node stage.mjs --check-git-proxy <url>
//
// The second form only checks a default proxy of git as the first takes it,
// and prints it as the manifest would have it (the GNUmakefile checks
// WASM_GIT_PROXY so, before it builds anything); it fails as the first
// would, naming WASM_GIT_PROXY.
//
// Every option also takes the form --name=value, which is the only way to
// give an empty value from a list of CMake (--git-proxy-default= for none).
// cmake/emscripten/stage.cmake runs it on every build.  It writes
//
//   <web files>                the files of --web (packaging/emscripten/web),
//                              with @BUILD@ replaced by the build id in the
//                              text files (for ?v=@BUILD@ cache busting)
//   pharo-web.js, .wasm        the web module (--module)
//   THIRD-PARTY-NOTICES.txt    the licences of the libraries in the VM
//                              (--notices, which stage.cmake assembles),
//                              copied unchanged; index.html and world.html
//                              link it
//   image/Pharo.image.gz       the default image, its changes and its
//   image/Pharo.changes.gz     .sources, gzipped (zlib level 9,
//   image/<name>.sources.gz    deterministic); the image of the world from
//                              --world-image when it has one, the stock one
//                              otherwise
//   image/<id>/...             the same three files of each extra image
//                              (--extra-image <id>=<stock dir>): of its world
//                              image (--extra-world-image <id>=<dir>) when it
//                              has one, of the stock one otherwise
//   image/sources/<name>.gz    each --extra-sources file, a .sources that no
//                              image of the site has: an image saved in the
//                              browser from an image that the site served
//                              before finds the source of its methods there
//                              (one that an image has already is not staged
//                              again)
//   st/<file>                  the --st files, and with the world image its
//                              OSWindow-Web.st and the --world-st files
//   manifest.json              {build, extraSources, ffi, files, fonts, git,
//                              gitHttp, gitProxyDefault, image, imageId,
//                              images, libraries, memory64, st,
//                              webPackage, world}
//
// The build id is the git sha, a dash and the first 12 hex digits of a
// SHA256 over everything staged and every key of the manifest (the build
// of each image too), so it changes whenever a staged file or the manifest
// does: the pages and the worker fetch manifest.json?v=<build>.
// manifest.json describes the build, and lists what the worker writes into
// /pharo:
//
//   extraSources  [{gzSize, path, sha256, size, url}] of the --extra-sources
//          files (as files), sorted by path; no key when there is none
//   ffi    true when the VM has the FFI (--ffi 1, from FEATURE_FFI, that is
//          WASM_FFI): make wasm-check-browser runs ffi.spec.mjs only then
//   files  [{gzSize, path, sha256, size, url}] of the default image, sorted
//          by path: path is the name under /pharo, url the gzipped file
//          relative to web/ (the worker adds ?v=<build>), size and sha256
//          those of the file, and gzSize the size of the download
//   fonts  "freetype" or "bitmap" (--fonts, from PHARO_WASM_HAS_FREETYPE):
//          the fonts that the preparation of an image for the world sets
//          up, the build's (webimage.cmake) and the page's
//          (web-bootstrap.st), which the page names in its notice
//   git    true when the VM has libgit2 (--libgit2 1, from
//          PHARO_WASM_HAS_LIBGIT2, that is WASM_LIBGIT2 with the FFI): make
//          wasm-check-browser runs git.spec.mjs only then
//   gitHttp  true when it also has the smart-HTTP transport of libgit2
//          (--libgit2-http 1, from PHARO_WASM_HAS_LIBGIT2_HTTP, that is
//          WASM_LIBGIT2_HTTP), whose CORS proxy the pages' Settings hold;
//          never true without git
//   gitProxyDefault  the site's default CORS proxy of git, which the pages
//          use unless the user chose another or none (--git-proxy-default,
//          from WASM_GIT_PROXY: an http: or https: URL without credentials
//          or fragment, a bare origin with its /), "" for none; only with
//          gitHttp
//   image  the name of the image to boot, "Pharo.image"
//   imageId  the id of the default image in images: --image-id, else
//          p<major> of the name of its .sources (p15 for
//          Pharo15.0-64bit-b85960f.sources), else "default"
//   images  the catalog, one entry per image, also when there is one:
//          {files, id, label, pharo, pharoBuild, webPackage, world}, and
//          default: true on the default one, whose files are those of the
//          key files.  An id is 1 to 16 lower-case letters, digits and
//          dashes (not first); pharo is "13.1" for the .sources
//          Pharo13.1-64bit-d7c6f76.sources, null for a .sources of another
//          name; label "Pharo 13.1", "Pharo 14" (for 14.0), or the id;
//          pharoBuild the build of the stock image, from the name of the zip
//          that download.cmake unpacked (<stock dir>.stamp), null when
//          unknown; world and webPackage as the keys below, for that image
//          (webPackage 0 for a stock image).  Sorted by pharo, numerically,
//          those without one last, then by id
//   libraries  ["libcairo.so.2", ...], sorted: the --library-file names, the
//          placeholders of the libraries of the FFI (the FILES of
//          pharo_wasm_ffi_library, which stage.cmake passes).  The worker
//          writes each one empty into /pharo, the directory of the VM,
//          before the boot, for the image to find the library there as a
//          file (FFIUnix64LibraryFinder); the VM has it built in
//   st     ["st/<file>", ...]: each file is at that url, and goes to /pharo/st
//   webPackage  the version of the package OSWindow-Web of this build, an
//          integer: OSWebDriver class>>packageVersion, read from
//          --web-package, the source of the class.  The worker prepares
//          again an image whose version is older (it finds it in the image
//          as the symbol of OSWebDriver class>>packageMarker, which must be
//          #OSWindowWebPackage<version>).  The source is the single place
//          where the version is written
//   world  true when the default image is the image of the world
//
// The checks: the ids are unique, differ from the default one, and none is
// sources, the directory of the --extra-sources files (an extra image may be
// named default: each image lists its files in a list of its own, never
// looked up by id); two images whose .sources have one name have the same
// file; an --extra-sources file of the name of a .sources of an image is
// that file; and the OSWindow-Web.st of each extra world image is the
// default world's, the only one staged (st/OSWindow-Web.st, which the pages
// file in to prepare a stock image), so there is no extra world image
// without a default one.
//
// Only files whose contents change are rewritten, and a file is gzipped
// again only when its SHA256 changes (the gzipped files of every list of
// the manifest before count).  Files of an earlier staging that are not
// staged any more are removed.

import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readdirSync, readFileSync, renameSync, rmdirSync,
         statSync, unlinkSync, writeFileSync } from 'node:fs';
import { basename, dirname, extname, join, relative, resolve, sep } from 'node:path';
import { promisify } from 'node:util';
import { gzip } from 'node:zlib';

const TEXT_EXTENSIONS = new Set(['.css', '.htm', '.html', '.js', '.json', '.mjs', '.svg', '.txt',
                                 '.webmanifest']);
const IMAGE = 'Pharo.image';
const CHANGES = 'Pharo.changes';
const WORLD_IMAGE = 'Pharo-web.image';
const NOTICES = 'THIRD-PARTY-NOTICES.txt';

const fail = (message) => {
  process.stderr.write(`stage.mjs: ${message}\n`);
  process.exit(1);
};

// The id of an image of the catalog, a name in its URLs (image/<id>/)
const ID = /^[a-z0-9][a-z0-9-]{0,15}$/;
// The name of the .sources of a Pharo release: Pharo13.1-64bit-d7c6f76.sources
const SOURCES_NAME = /^Pharo(\d+)\.(\d+)-(?:32|64)bit-([0-9a-f]+)\.sources$/;

// The default proxy of git, checked as git-proxy.js checks what the user
// types (parse): http: or https:, no user name or password, no fragment; as
// the browser writes it (https:proxy.example/git is
// https://proxy.example/git), a bare origin with its /, which the host of
// the repository follows.  '' is none.  what names it in a failure
const gitProxyOf = (text, what = '--git-proxy-default') => {
  const s = String(text || '').trim();
  if (!s) return '';
  let u;
  try { u = new URL(s); } catch (e) { fail(`${what} ${s} is not a URL`); }
  if (u.protocol !== 'http:' && u.protocol !== 'https:') fail(`${what} ${s} is not an http:// or https:// URL`);
  if (u.username || u.password) fail(`${what} ${s} has a user name or password`);
  if (u.hash || s.indexOf('#') >= 0) fail(`${what} ${s} has a #fragment`);
  return u.pathname === '/' && !u.search ? u.origin + '/' : u.href;
};

// <id>=<dir> of --extra-image and --extra-world-image
const idAndDir = (name, value) => {
  const m = /^([^=]*)=(.+)$/.exec(value);
  if (!m) fail(`${name} ${value} is not <id>=<dir>`);
  if (!ID.test(m[1])) fail(`${name} ${value}: the id ${m[1]} is not 1 to 16 lower-case letters, digits and dashes (not first)`);
  return [m[1], m[2]];
};

const parseArguments = (argv) => {
  const options = { st: [], worldSt: [], libraries: [], memory64: '2', git: '', ffi: '0', fonts: 'bitmap',
                    libgit2: '0', libgit2Http: '0', imageId: '', gitProxyDefault: '',
                    extraImages: [], extraWorldImages: [], extraSources: [] };
  const single = { '--out': 'out', '--web': 'web', '--module': 'module', '--memory64': 'memory64',
                   '--git': 'git', '--ffi': 'ffi', '--fonts': 'fonts', '--notices': 'notices',
                   '--libgit2': 'libgit2', '--libgit2-http': 'libgit2Http',
                   '--stock-image': 'stockImage', '--web-package': 'webPackage', '--world-image': 'worldImage',
                   '--image-id': 'imageId', '--git-proxy-default': 'gitProxyDefault' };
  const many = { '--st': 'st', '--world-st': 'worldSt', '--library-file': 'libraries',
                 '--extra-image': 'extraImages', '--extra-world-image': 'extraWorldImages',
                 '--extra-sources': 'extraSources' };
  for (let i = 0; i < argv.length; i++) {
    // (--name value, or --name=value, which can be empty)
    let name = argv[i], value;
    const eq = name.indexOf('=');
    if (name.startsWith('--') && eq > 0) {
      value = name.slice(eq + 1);
      name = name.slice(0, eq);
    } else {
      if (i + 1 >= argv.length) fail(`bad argument ${name}`);
      value = argv[++i];
    }
    if (!(name in single || name in many)) fail(`bad argument ${name}`);
    if (name in single) options[single[name]] = value;
    else options[many[name]].push(value);
  }
  for (const required of ['out', 'module', 'stockImage', 'webPackage'])
    if (!options[required]) fail(`--${required.replace(/[A-Z]/g, (c) => '-' + c.toLowerCase())} is missing`);
  if (!/^[12]$/.test(options.memory64)) fail(`--memory64 must be 1 or 2, not ${options.memory64}`);
  if (!/^[01]$/.test(options.ffi)) fail(`--ffi must be 0 or 1, not ${options.ffi}`);
  if (!/^(freetype|bitmap)$/.test(options.fonts)) fail(`--fonts must be freetype or bitmap, not ${options.fonts}`);
  if (!/^[01]$/.test(options.libgit2)) fail(`--libgit2 must be 0 or 1, not ${options.libgit2}`);
  if (!/^[01]$/.test(options.libgit2Http)) fail(`--libgit2-http must be 0 or 1, not ${options.libgit2Http}`);
  if (!existsSync(options.webPackage)) fail(`--web-package ${options.webPackage} does not exist`);
  if (options.notices && !existsSync(options.notices)) fail(`--notices ${options.notices} does not exist`);
  // (a name in /pharo, which the worker writes)
  for (const name of options.libraries)
    if (!/^[A-Za-z0-9_+-][A-Za-z0-9._+-]*$/.test(name)) fail(`--library-file ${name} is not the name of a file`);
  options.libraries = [...new Set(options.libraries)].sort();
  if (options.imageId && !ID.test(options.imageId))
    fail(`--image-id ${options.imageId} is not 1 to 16 lower-case letters, digits and dashes (not first)`);
  options.gitProxyDefault = gitProxyOf(options.gitProxyDefault);
  // The extra images, in their order, each with its world image or null
  const extras = new Map();
  for (const value of options.extraImages) {
    const [id, dir] = idAndDir('--extra-image', value);
    if (extras.has(id)) fail(`--extra-image ${id} is given twice`);
    if (id == 'sources') fail(`--extra-image ${id}: image/sources/ holds the --extra-sources files, name it otherwise`);
    extras.set(id, { id, stockDir: dir, worldDir: null });
  }
  for (const value of options.extraWorldImages) {
    const [id, dir] = idAndDir('--extra-world-image', value);
    if (!extras.has(id)) fail(`--extra-world-image ${id} has no --extra-image ${id}`);
    if (extras.get(id).worldDir) fail(`--extra-world-image ${id} is given twice`);
    extras.get(id).worldDir = dir;
  }
  options.extras = [...extras.values()];
  for (const file of options.extraSources)
    if (!/^[^/]+\.sources$/.test(basename(file)) || !existsSync(file))
      fail(`--extra-sources ${file} does not exist, or is no .sources`);
  return options;
};

const sha256 = (data) => createHash('sha256').update(data).digest('hex');

const walk = (dir, prefix = '') => {
  if (!existsSync(dir)) return [];
  const files = [];
  for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name < b.name ? -1 : 1)) {
    if (entry.name.startsWith('.')) continue;
    const rel = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isDirectory()) files.push(...walk(join(dir, entry.name), rel));
    else if (entry.isFile()) files.push(rel);
  }
  return files;
};

// The one image of dir, its changes and its .sources
const imageFiles = (dir) => {
  const names = existsSync(dir) ? readdirSync(dir) : [];
  const images = names.filter((name) => name.endsWith('.image'));
  const sources = names.filter((name) => name.endsWith('.sources'));
  if (images.length != 1) fail(`expected one .image in ${dir}, found ${images.length}`);
  const changes = images[0].replace(/\.image$/, '.changes');
  if (!names.includes(changes) || sources.length != 1)
    fail(`${dir} lacks the .changes of ${images[0]} or has not exactly one .sources`);
  return { image: join(dir, images[0]), changes: join(dir, changes), sources: join(dir, sources[0]) };
};

// The version of OSWindow-Web, from the Tonel source of OSWebDriver: the
// integer that OSWebDriver class>>packageVersion answers, checked against
// the symbol of OSWebDriver class>>packageMarker.  Each method is
// 'OSWebDriver class >> <selector> [' up to the line ']', and answers a
// literal, after its comment
const webPackageOf = (file) => {
  const source = readFileSync(file, 'utf8').replace(/\r\n?/g, '\n');
  const answer = (selector) => {
    const start = source.indexOf(`\nOSWebDriver class >> ${selector} [\n`);
    if (start < 0) fail(`${file} has no method OSWebDriver class>>${selector}`);
    const end = source.indexOf('\n]', start + 1);
    // (comments are between double quotes, which a comment doubles)
    const body = source.slice(source.indexOf('[\n', start) + 2, end < 0 ? undefined : end).replace(/"(?:[^"]|"")*"/g, ' ');
    const m = /^\s*\^\s*(\S+?)\s*\.?\s*$/.exec(body);
    if (!m) fail(`OSWebDriver class>>${selector} of ${file} answers no literal: ${body.trim().slice(0, 80)}`);
    return m[1];
  };
  const version = answer('packageVersion'), marker = answer('packageMarker');
  if (!/^[1-9][0-9]*$/.test(version))
    fail(`OSWebDriver class>>packageVersion of ${file} answers ${version}, not a positive integer`);
  if (marker != `#OSWindowWebPackage${version}`)
    fail(`OSWebDriver class>>packageMarker of ${file} answers ${marker}, not #OSWindowWebPackage${version} ` +
         `(it must change with packageVersion)`);
  return Number(version);
};

const sortKeys = (value) => {
  if (Array.isArray(value)) return value.map(sortKeys);
  if (value && typeof value == 'object')
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, sortKeys(value[key])]));
  return value;
};

// "13.1" for Pharo13.1-64bit-d7c6f76.sources, null for a name of no release
const pharoOf = (sourcesName) => {
  const m = SOURCES_NAME.exec(sourcesName);
  return m ? `${m[1]}.${m[2]}` : null;
};
// "Pharo 14" for 14.0, "Pharo 13.1" for 13.1
const labelOf = (pharo, id) => {
  if (!pharo) return id;
  const [major, minor] = pharo.split('.');
  return Number(minor) == 0 ? `Pharo ${major}` : `Pharo ${pharo}`;
};
// The build of the stock image of dir, from the name of the zip that
// download.cmake unpacked there, which <dir>.stamp records ("<sha256>
// <zip>"): 50 for Pharo15.0-SNAPSHOT.build.50.sha.b85960fcc6.arch.64bit.zip,
// else null
const pharoBuildOf = (dir) => {
  const stamp = resolve(dir) + '.stamp';
  if (!existsSync(stamp)) return null;
  const m = /^[0-9a-f]* (.+?)\n?$/.exec(readFileSync(stamp, 'utf8'));
  const build = m && /\.build\.(\d+)\.sha\./.exec(basename(m[1]));
  return build ? Number(build[1]) : null;
};
// The order of the catalog: by release, numerically, those of no release
// last, then by id
const catalogOrder = (a, b) => {
  const va = a.pharo && a.pharo.split('.').map(Number), vb = b.pharo && b.pharo.split('.').map(Number);
  if (va && vb) {
    if (va[0] != vb[0]) return va[0] - vb[0];
    if (va[1] != vb[1]) return va[1] - vb[1];
  } else if (va || vb) {
    return va ? -1 : 1;
  }
  return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
};

const main = async () => {
  const options = parseArguments(process.argv.slice(2));
  const out = resolve(options.out);
  const webPackage = webPackageOf(options.webPackage);

  // Each file is read once, when first asked for
  const contents = new Map();
  const read = (source) => {
    if (!contents.has(source)) {
      const data = readFileSync(source);
      contents.set(source, { data, sha256: sha256(data) });
    }
    return contents.get(source);
  };

  // The images of the catalog, the default one first: the world's of each
  // when there is one
  const world = !!options.worldImage && existsSync(join(options.worldImage, WORLD_IMAGE));
  // (each with the list of its files in the manifest, listed: the list
  // belongs to the image, so no id, not even "default", shares the default's)
  const catalog = [{ id: null, prefix: 'image/', stockDir: options.stockImage, world, listed: [],
                     files: imageFiles(world ? options.worldImage : options.stockImage) }];
  for (const extra of options.extras) {
    const extraWorld = !!extra.worldDir && existsSync(join(extra.worldDir, WORLD_IMAGE));
    if (extraWorld) {
      // (the pages file in st/OSWindow-Web.st, the default world's, into any
      // stock image they prepare)
      if (!world) fail(`--extra-world-image ${extra.id}: there is no world image of the default image (--world-image)`);
      const own = join(extra.worldDir, 'OSWindow-Web.st'), staged = join(options.worldImage, 'OSWindow-Web.st');
      if (!existsSync(own) || !existsSync(staged) || !readFileSync(own).equals(readFileSync(staged)))
        fail(`${own} is not ${staged}: the images of the world must be prepared with the same OSWindow-Web`);
    }
    catalog.push({ id: extra.id, prefix: `image/${extra.id}/`, stockDir: extra.stockDir, listed: [],
                   world: extraWorld, files: imageFiles(extraWorld ? extra.worldDir : extra.stockDir) });
  }
  for (const image of catalog) {
    image.sourcesName = basename(image.files.sources);
    image.pharo = pharoOf(image.sourcesName);
    image.pharoBuild = pharoBuildOf(image.stockDir);
  }
  // The id of the default image: --image-id, else p<major> of its .sources
  const [first] = catalog;
  first.id = options.imageId || (first.pharo ? `p${first.pharo.split('.')[0]}` : 'default');
  for (const image of catalog) {
    if (image != first && image.id == first.id)
      fail(`--extra-image ${image.id} has the id of the default image (--image-id)`);
    image.label = labelOf(image.pharo, image.id);
  }
  // Two .sources of one name are the same file
  const sourcesByName = new Map();
  for (const image of catalog) {
    const known = sourcesByName.get(image.sourcesName);
    if (known && read(known.files.sources).sha256 != read(image.files.sources).sha256)
      fail(`the images ${known.id} and ${image.id} have two different ${image.sourcesName}`);
    if (!known) sourcesByName.set(image.sourcesName, image);
  }

  // What is staged: [path in web/, source, kind, file], kind being text
  // (@BUILD@ replaced), copy or gzip, and file, for gzip, {path, listed}:
  // its name in /pharo and the list of the manifest that lists it (the
  // listed of its image, or extraSourcesListed)
  const entries = [];
  const extraSourcesListed = [];
  if (options.web)
    for (const rel of walk(options.web))
      entries.push([rel, join(options.web, rel), TEXT_EXTENSIONS.has(extname(rel)) ? 'text' : 'copy']);
  const module = resolve(options.module);
  entries.push([basename(module), module, 'copy']);
  entries.push([basename(module).replace(/\.js$/, '.wasm'), module.replace(/\.js$/, '.wasm'), 'copy']);
  // (copied: the licence texts are staged as they are, without @BUILD@)
  if (options.notices) entries.push([NOTICES, resolve(options.notices), 'copy']);
  else process.stderr.write(`stage.mjs: warning: no --notices, web/ goes without the ${NOTICES} that the pages link\n`);
  for (const image of catalog) {
    const files = [[IMAGE, image.files.image], [CHANGES, image.files.changes], [image.sourcesName, image.files.sources]];
    for (const [name, source] of files)
      entries.push([`${image.prefix}${name}.gz`, source, 'gzip', { path: name, listed: image.listed }]);
  }
  for (const name of options.libraries)
    if (name == IMAGE || name == CHANGES || sourcesByName.has(name))
      fail(`--library-file ${name} is the name of a file of an image`);
  // The other .sources, but those the catalog serves already
  const extraSources = new Map();
  for (const file of options.extraSources) {
    const name = basename(file);
    const known = sourcesByName.has(name) ? sourcesByName.get(name).files.sources : extraSources.get(name);
    if (known) {
      if (read(known).sha256 != read(file).sha256) fail(`--extra-sources ${file} is not the ${name} of the site`);
      continue;
    }
    extraSources.set(name, file);
    entries.push([`image/sources/${name}.gz`, file, 'gzip', { path: name, listed: extraSourcesListed }]);
  }
  const st = [...options.st];
  if (world) st.push(join(options.worldImage, 'OSWindow-Web.st'), ...options.worldSt);
  for (const file of st) {
    if (existsSync(file)) entries.push([`st/${basename(file)}`, file, 'copy']);
    else process.stderr.write(`stage.mjs: warning: ${file} does not exist, web/ goes without it\n`);
  }

  const inputs = new Map();
  for (const [rel, source] of entries) {
    if (inputs.has(rel) || rel == 'manifest.json') fail(`${source} would be staged as ${rel} twice`);
    inputs.set(rel, read(source));
  }
  const ffi = options.ffi == '1';
  // (the transport is a part of libgit2)
  const git = options.libgit2 == '1', gitHttp = git && options.libgit2Http == '1';
  const digest = sha256([`ffi ${ffi}`, `fonts ${options.fonts}`, `git ${git}`, `gitHttp ${gitHttp}`,
                         `gitProxyDefault ${options.gitProxyDefault}`, `imageId ${first.id}`,
                         ...catalog.map((image) => `image ${image.id} ${image.label} ${image.pharo} ` +
                                        `${image.pharoBuild} ${image.world}`),
                         `extraSources ${[...extraSources.keys()].sort().join(' ')}`,
                         `libraries ${options.libraries.join(' ')}`,
                         `memory64 ${options.memory64}`,
                         `webPackage ${webPackage}`, `world ${world}`,
                         ...[...inputs].map(([rel, { sha256 }]) => `${rel} ${sha256}`).sort()].join('\n'));
  const build = (options.git ? `${options.git}-` : '') + digest.slice(0, 12);

  // A directory that is not empty must be one this script staged
  const manifestPath = join(out, 'manifest.json');
  let previous = null;
  if (existsSync(manifestPath)) {
    try { previous = JSON.parse(readFileSync(manifestPath, 'utf8')); } catch (e) { previous = null; }
  } else if (existsSync(out) && readdirSync(out).length > 0) {
    fail(`${out} is not empty and has no manifest.json: not a directory to stage into`);
  }
  const previousFiles = new Map([...((previous && previous.files) || []),
                                  ...((previous && previous.images) || []).flatMap((image) => image.files || []),
                                  ...((previous && previous.extraSources) || [])].map((file) => [file.url, file]));

  let written = 0;
  const write = (rel, data) => {
    const path = join(out, rel);
    if (existsSync(path) && statSync(path).size == data.length && readFileSync(path).equals(data)) return;
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(`${path}.stage-tmp`, data);
    renameSync(`${path}.stage-tmp`, path);
    written++;
  };

  // The files of each list of the manifest
  const compressions = [];
  for (const [rel, source, kind, meta] of entries) {
    const { data, sha256: hash } = inputs.get(rel);
    if (kind == 'text') {
      write(rel, Buffer.from(data.toString('utf8').replaceAll('@BUILD@', build), 'utf8'));
    } else if (kind == 'copy') {
      write(rel, data);
    } else {
      const file = { path: meta.path, url: rel, size: data.length, sha256: hash, gzSize: 0 };
      meta.listed.push(file);
      const old = previousFiles.get(rel);
      const path = join(out, rel);
      if (old && old.sha256 == hash && existsSync(path) && statSync(path).size == old.gzSize) {
        file.gzSize = old.gzSize;
      } else {
        compressions.push(promisify(gzip)(data, { level: 9 }).then((gz) => {
          file.gzSize = gz.length;
          write(rel, gz);
        }));
      }
    }
  }
  await Promise.all(compressions);

  const byPath = (a, b) => a.path < b.path ? -1 : 1;
  for (const files of [...catalog.map((image) => image.listed), extraSourcesListed]) files.sort(byPath);
  const images = catalog.map((image) => ({
    ...(image == first ? { default: true } : {}),
    files: image.listed,
    id: image.id,
    label: image.label,
    pharo: image.pharo,
    pharoBuild: image.pharoBuild,
    webPackage: image.world ? webPackage : 0,
    world: image.world,
  })).sort(catalogOrder);
  const manifest = sortKeys({
    build,
    ...(extraSourcesListed.length ? { extraSources: extraSourcesListed } : {}),
    ffi,
    files: first.listed,
    fonts: options.fonts,
    git,
    gitHttp,
    ...(gitHttp ? { gitProxyDefault: options.gitProxyDefault } : {}),
    image: IMAGE,
    imageId: first.id,
    images,
    libraries: options.libraries,
    memory64: Number(options.memory64),
    st: entries.filter(([rel]) => rel.startsWith('st/')).map(([rel]) => rel).sort(),
    webPackage,
    world,
  });
  write('manifest.json', Buffer.from(JSON.stringify(manifest, null, 2) + '\n', 'utf8'));

  // What an earlier staging left
  let removed = 0;
  const staged = new Set([...entries.map(([rel]) => rel), 'manifest.json']);
  const prune = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) {
        prune(path);
        if (readdirSync(path).length == 0) rmdirSync(path);
      } else if (!staged.has(relative(out, path).split(sep).join('/'))) {
        unlinkSync(path);
        removed++;
      }
    }
  };
  prune(out);

  if (written || removed)
    process.stdout.write(`stage.mjs: ${out}: ${written} file(s) written, ${removed} removed, build ${build}\n`);
};

if (process.argv[2] === '--check-git-proxy') process.stdout.write(gitProxyOf(process.argv[3], 'WASM_GIT_PROXY') + '\n');
else main().catch((e) => fail((e && e.stack) || e));
