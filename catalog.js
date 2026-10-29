// catalog.js - the images that a site offers, as its manifest.json lists them
//
// UMD: the global PharoCatalog in a worker (importScripts) and in the pages
// (<script>), a CommonJS module in node (the harnesses).  It uses no DOM,
// and it is the only reader of the catalog of manifest.json: vm-worker.js,
// world.js, page.js and images.js ask it, never manifest.images itself.
//
// A site serves one or more Pharo images (stage.mjs: the default one, and
// those of WASM_EXTRA_IMAGES).  manifest.images lists them, sorted by
// version, each entry
//
//   {id, label, pharo, pharoBuild, world, webPackage, files, default?}
//
// id is the catalog id (ID, p15), label what the pages show (Pharo 13.1,
// Pharo 14), pharo the version that the name of its .sources says ("13.1",
// or null), pharoBuild the build number of its zip on files.pharo.org (or
// null), world whether it is prepared for the world, webPackage its version
// of OSWindow-Web (0 when not prepared), and files its Pharo.image,
// Pharo.changes and .sources, each {path, url, size, gzSize, sha256}: path
// is the name it has in /pharo, url where the site serves it.  Exactly one
// entry is the default (default: true), the image that the top-level keys
// of the manifest (files, world, webPackage) still describe, for the
// readers of before the catalog.  manifest.extraSources lists the .sources
// the site serves for the images saved on an earlier site only (the same
// file objects), whose own .sources it no longer has otherwise.
//
// A manifest of before the catalog (no images) has one entry, made of its
// top-level keys: the id imageId, else 'default', and the version that the
// name of its .sources says.
//
//   PharoCatalog.images(manifest)         the entries
//   PharoCatalog.defaultEntry(manifest)   the default entry
//   PharoCatalog.find(manifest, id)       the entry of that id, or null
//   PharoCatalog.sourcesOf(entry)         the file object of its .sources, or null
//   PharoCatalog.sourcesFile(manifest, name)
//                                         the .sources named so that the site
//                                         serves (an entry's or an extra one), or null
//   PharoCatalog.entryOfFile(manifest, file)
//                                         the entry whose files have that one, or null
//   PharoCatalog.pharoOf(sourcesName)     '13.1' for Pharo13.1-64bit-d7c6f76.sources, or null
//   PharoCatalog.labelOf(pharo)           'Pharo 13.1', 'Pharo 14' (a minor 0), 'Pharo' (null)
//   PharoCatalog.downloadSize(entry)      the bytes it downloads (gzSize)
//   PharoCatalog.ID                       what a catalog id matches

(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.PharoCatalog = factory();
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  const ID = /^[a-z0-9][a-z0-9-]{0,15}$/;
  // The name of the .sources of a Pharo: its version, its word size and the
  // commit it was built from
  const SOURCES = /^Pharo(\d+)\.(\d+)-(?:32|64)bit-([0-9a-f]+)\.sources$/;

  const isSources = f => !!f && typeof f.path === 'string' && /\.sources$/.test(f.path);

  function pharoOf(sourcesName) {
    const m = SOURCES.exec(String(sourcesName || ''));
    return m ? Number(m[1]) + '.' + Number(m[2]) : null;
  }

  function labelOf(pharo) {
    const m = /^(\d+)\.(\d+)$/.exec(String(pharo || ''));
    if (!m) return 'Pharo';
    return 'Pharo ' + (Number(m[2]) === 0 ? m[1] : m[1] + '.' + m[2]);
  }

  // The entries; one made of the top-level keys for a manifest without them
  function images(manifest) {
    if (!manifest) return [];
    if (Array.isArray(manifest.images) && manifest.images.length) return manifest.images;
    const files = Array.isArray(manifest.files) ? manifest.files : [];
    const sources = files.find(isSources);
    const pharo = sources ? pharoOf(sources.path) : null;
    return [{
      id: typeof manifest.imageId === 'string' && ID.test(manifest.imageId) ? manifest.imageId : 'default',
      default: true, label: labelOf(pharo), pharo, pharoBuild: null, files,
      world: !!manifest.world, webPackage: manifest.world ? manifest.webPackage : 0,
    }];
  }

  function defaultEntry(manifest) {
    const all = images(manifest);
    return all.find(e => e.default) || all[0] || null;
  }

  function find(manifest, id) {
    return images(manifest).find(e => e.id === id) || null;
  }

  function sourcesOf(entry) {
    return (entry && Array.isArray(entry.files) && entry.files.find(isSources)) || null;
  }

  function sourcesFile(manifest, name) {
    if (!name) return null;
    for (const e of images(manifest)) {
      const f = (e.files || []).find(x => isSources(x) && x.path === name);
      if (f) return f;
    }
    const extra = (manifest && Array.isArray(manifest.extraSources)) ? manifest.extraSources : [];
    return extra.find(x => isSources(x) && x.path === name) || null;
  }

  function entryOfFile(manifest, file) {
    if (!file) return null;
    return images(manifest).find(e => (e.files || []).some(f => f === file ||
      (f.path === file.path && f.url === file.url))) || null;
  }

  const downloadSize = entry => ((entry && entry.files) || []).reduce((n, f) => n + (f.gzSize || f.size || 0), 0);

  return { images, defaultEntry, find, sourcesOf, sourcesFile, entryOfFile, pharoOf, labelOf, downloadSize, ID };
});
