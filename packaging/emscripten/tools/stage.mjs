// stage.mjs - writes web/, the static site of the Pharo VM for WebAssembly
//
//   node stage.mjs --out <web dir> --module <pharo-web.js> --stock-image <dir>
//        --web-package <OSWebDriver.class.st>
//        [--web <dir>] [--memory64 1|2] [--git <sha>] [--ffi 0|1]
//        [--fonts freetype|bitmap] [--notices <file>] [--st <file>]...
//        [--library-file <name>]...
//        [--world-image <dir> [--world-st <file>]...]
//
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
//   image/Pharo.image.gz       the image, its changes and its .sources,
//   image/Pharo.changes.gz     gzipped (zlib level 9, deterministic); the
//   image/<name>.sources.gz    image of the world from --world-image when
//                              it has one, the stock one otherwise
//   st/<file>                  the --st files, and with the world image its
//                              OSWindow-Web.st and the --world-st files
//   manifest.json              {build, ffi, files, fonts, image, libraries,
//                              memory64, st, webPackage, world}
//
// The build id is the git sha, a dash and the first 12 hex digits of a
// SHA256 over everything staged, so it changes whenever a staged file does.
// manifest.json describes the build, and lists what the worker writes into
// /pharo:
//
//   ffi    true when the VM has the FFI (--ffi 1, from FEATURE_FFI, that is
//          WASM_FFI): make wasm-check-browser runs ffi.spec.mjs only then
//   files  [{gzSize, path, sha256, size, url}], sorted by path: path is the
//          name under /pharo, url the gzipped file relative to web/ (the
//          worker adds ?v=<build>), size and sha256 those of the file, and
//          gzSize the size of the download
//   fonts  "freetype" or "bitmap" (--fonts, from PHARO_WASM_HAS_FREETYPE):
//          the fonts that the preparation of an image for the world sets
//          up, the build's (webimage.cmake) and the page's
//          (web-bootstrap.st), which the page names in its notice
//   image  the name of the image to boot, "Pharo.image"
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
//   world  true when the image is the image of the world
//
// Only files whose contents change are rewritten, and a file is gzipped
// again only when its SHA256 changes.  Files of an earlier staging that are
// not staged any more are removed.

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

const parseArguments = (argv) => {
  const options = { st: [], worldSt: [], libraries: [], memory64: '2', git: '', ffi: '0', fonts: 'bitmap' };
  const single = { '--out': 'out', '--web': 'web', '--module': 'module', '--memory64': 'memory64',
                   '--git': 'git', '--ffi': 'ffi', '--fonts': 'fonts', '--notices': 'notices',
                   '--stock-image': 'stockImage', '--web-package': 'webPackage', '--world-image': 'worldImage' };
  const many = { '--st': 'st', '--world-st': 'worldSt', '--library-file': 'libraries' };
  for (let i = 0; i < argv.length; i++) {
    const name = argv[i];
    if (i + 1 >= argv.length || !(name in single || name in many)) fail(`bad argument ${name}`);
    const value = argv[++i];
    if (name in single) options[single[name]] = value;
    else options[many[name]].push(value);
  }
  for (const required of ['out', 'module', 'stockImage', 'webPackage'])
    if (!options[required]) fail(`--${required.replace(/[A-Z]/g, (c) => '-' + c.toLowerCase())} is missing`);
  if (!/^[12]$/.test(options.memory64)) fail(`--memory64 must be 1 or 2, not ${options.memory64}`);
  if (!/^[01]$/.test(options.ffi)) fail(`--ffi must be 0 or 1, not ${options.ffi}`);
  if (!/^(freetype|bitmap)$/.test(options.fonts)) fail(`--fonts must be freetype or bitmap, not ${options.fonts}`);
  if (!existsSync(options.webPackage)) fail(`--web-package ${options.webPackage} does not exist`);
  if (options.notices && !existsSync(options.notices)) fail(`--notices ${options.notices} does not exist`);
  // (a name in /pharo, which the worker writes)
  for (const name of options.libraries)
    if (!/^[A-Za-z0-9_+-][A-Za-z0-9._+-]*$/.test(name)) fail(`--library-file ${name} is not the name of a file`);
  options.libraries = [...new Set(options.libraries)].sort();
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

const main = async () => {
  const options = parseArguments(process.argv.slice(2));
  const out = resolve(options.out);

  // Which image: the world's when there is one
  const world = !!options.worldImage && existsSync(join(options.worldImage, WORLD_IMAGE));
  const image = imageFiles(world ? options.worldImage : options.stockImage);

  // What is staged: [path in web/, source, kind], kind being text (@BUILD@
  // replaced), copy or gzip
  const entries = [];
  if (options.web)
    for (const rel of walk(options.web))
      entries.push([rel, join(options.web, rel), TEXT_EXTENSIONS.has(extname(rel)) ? 'text' : 'copy']);
  const module = resolve(options.module);
  entries.push([basename(module), module, 'copy']);
  entries.push([basename(module).replace(/\.js$/, '.wasm'), module.replace(/\.js$/, '.wasm'), 'copy']);
  // (copied: the licence texts are staged as they are, without @BUILD@)
  if (options.notices) entries.push([NOTICES, resolve(options.notices), 'copy']);
  else process.stderr.write(`stage.mjs: warning: no --notices, web/ goes without the ${NOTICES} that the pages link\n`);
  const files = [[IMAGE, image.image], [CHANGES, image.changes], [basename(image.sources), image.sources]];
  for (const [name, source] of files) entries.push([`image/${name}.gz`, source, 'gzip']);
  for (const name of options.libraries)
    if (files.some(([file]) => file == name)) fail(`--library-file ${name} is the name of a file of the image`);
  const st = [...options.st];
  if (world) st.push(join(options.worldImage, 'OSWindow-Web.st'), ...options.worldSt);
  for (const file of st) {
    if (existsSync(file)) entries.push([`st/${basename(file)}`, file, 'copy']);
    else process.stderr.write(`stage.mjs: warning: ${file} does not exist, web/ goes without it\n`);
  }

  const inputs = new Map();
  for (const [rel, source] of entries) {
    if (inputs.has(rel) || rel == 'manifest.json') fail(`${source} would be staged as ${rel} twice`);
    const data = readFileSync(source);
    inputs.set(rel, { data, sha256: sha256(data) });
  }
  const ffi = options.ffi == '1';
  const webPackage = webPackageOf(options.webPackage);
  const digest = sha256([`ffi ${ffi}`, `fonts ${options.fonts}`, `libraries ${options.libraries.join(' ')}`,
                         `memory64 ${options.memory64}`, `webPackage ${webPackage}`, `world ${world}`,
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
  const previousFiles = new Map(((previous && previous.files) || []).map((file) => [file.url, file]));

  let written = 0;
  const write = (rel, data) => {
    const path = join(out, rel);
    if (existsSync(path) && statSync(path).size == data.length && readFileSync(path).equals(data)) return;
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(`${path}.stage-tmp`, data);
    renameSync(`${path}.stage-tmp`, path);
    written++;
  };

  const manifestFiles = [];
  const compressions = [];
  for (const [rel, source, kind] of entries) {
    const { data, sha256: hash } = inputs.get(rel);
    if (kind == 'text') {
      write(rel, Buffer.from(data.toString('utf8').replaceAll('@BUILD@', build), 'utf8'));
    } else if (kind == 'copy') {
      write(rel, data);
    } else {
      const file = { path: rel.replace(/^image\//, '').replace(/\.gz$/, ''), url: rel, size: data.length,
                     sha256: hash, gzSize: 0 };
      manifestFiles.push(file);
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

  const manifest = sortKeys({
    build,
    ffi,
    files: manifestFiles.sort((a, b) => a.path < b.path ? -1 : 1),
    fonts: options.fonts,
    image: IMAGE,
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

main().catch((e) => fail((e && e.stack) || e));
