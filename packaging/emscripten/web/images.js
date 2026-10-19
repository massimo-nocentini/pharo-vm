// images.js - the Images dialog of the pages: the images to start, fresh or saved
//
// UMD: the global PharoImages of the pages (<script>: index.html and
// world.html), a CommonJS module in node.  It needs the DOM, catalog.js
// (PharoCatalog) and vm-storage.js (a store of PharoStorage.open()).
//
//   const images = PharoImages.create({
//     doc,              the document (default document)
//     store,            PharoStorage.open(), or null when the page has none
//     manifest,         the manifest of the site, or () => it (null while
//                       unknown: no fresh images then)
//     current,          () => the choice of the page's VM: {slot}, {fresh}, {}
//     running,          () => the slot that the page's VM runs, or null
//     onOpen(choice),   start {fresh: id} or {slot}: answers false when the
//                       page did not (the user kept what runs), and the
//                       dialog then stays
//     onOpenFile(),     Open a file of the user's own (the page's Open)
//     onDeleteRunning(slot),
//                       delete the slot that the page's VM runs (the page
//                       resets its VM), or every slot when slot is null
//                       (Delete all); answers a promise
//     onChange(),       optional: a slot was renamed or deleted here
//     otherPage: {label, href: choice => url},
//                       the other page of the site (the Console from the
//                       world, the world from the Console), with a link per
//                       saved image
//     launcher })       true: the dialog opens as the launcher by default
//   images.open(note, launcher)   shows it (note: a line above, or null)
//   images.close(), images.refresh()
//
// The dialog, <dialog id="images-dialog"> (title "Images", #images-title),
// is made once, at the end of the body, with its style (#images-style, in
// the tokens of the page: --surface, --border, ... with fallbacks):
//
//   #images-note        a line of the page (That saved image is not in this
//                       browser), or why there are no saved images here
//   #images-fresh       "Start fresh": a button [data-fresh=<id>] per image of
//                       the catalog (PharoCatalog.images), its label, then
//                       its build and the size of its download, the default
//                       one said so
//   #images-saved       "Saved in this browser": a <li data-slot=<slot>> per
//                       slot (store.list(), newest first), its name, its
//                       Pharo, when it was saved, its size and the files it
//                       keeps, and its buttons [data-act=open] (a "Running
//                       here" badge instead for the running one),
//                       [data-act=rename] (prompt), [data-act=delete]
//                       (confirm), and the link [data-act=other] to the
//                       other page on it; or, with none, what they would be
//   #images-open-file   "Open a file…" (onOpenFile)
//   #images-usage       what this site uses of the browser's storage
//                       (store.estimate())
//   #images-persist     asks the browser to keep the images from its cleanup
//                       (navigator.storage.persist(), from this click), when
//                       it can and does not yet
//   #images-delete-all  confirm, then onDeleteRunning(null)
//   #images-close
//
// The launcher is the dialog for a first visit to the world page of a site
// of several images, where no VM runs until the user chose one: it has no
// Close, Escape does not close it, and the default image of the catalog has
// the focus, so that Enter starts it.  (A browser stops honouring the
// cancel event's preventDefault after a few Escapes in a row: the launcher
// then shows itself again as soon as it closed.)  The list is read again
// at each open() and when the window gets the focus back (another tab may
// have saved meanwhile).  Its rows are made again then, and after a rename
// or a delete: the focus, which would fall to the body (where the world
// page takes keys for the image), goes to the same button of the same row,
// else to the first one of the list, or of the dialog.

(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.PharoImages = factory();
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  const STYLE = `
#images-dialog {
  width: min(560px, calc(100vw - 32px));
  max-height: calc(100dvh - 32px);
  padding: 0;
  border: 1px solid var(--border, #d7dde4);
  border-radius: var(--radius, 10px);
  background: var(--surface, #fff);
  color: var(--text, #1b1f24);
  box-shadow: var(--float, var(--shadow, 0 8px 28px rgb(15 30 50 / .14)));
  overflow: auto;
  font: 15px/1.5 var(--sans, system-ui, sans-serif);
}
#images-dialog::backdrop { background: var(--scrim, rgb(15 30 50 / .32)); }
#images-dialog [hidden] { display: none !important; }
#images-dialog .images-body { display: flex; flex-direction: column; gap: 12px; padding: 18px 18px 14px; }
#images-dialog h2 { margin: 0; font-size: 16px; font-weight: 650; }
#images-dialog h3 { margin: 0 0 6px; font-size: 12.5px; font-weight: 650; letter-spacing: .02em; text-transform: uppercase;
  color: var(--muted, #5b6471); }
#images-dialog p { margin: 0; }
#images-note { padding: 8px 10px; border-radius: 8px; background: var(--accent-soft, #e2eefa); font-size: 13.5px;
  overflow-wrap: anywhere; }
#images-dialog .images-btn {
  appearance: none; display: inline-flex; align-items: center; justify-content: center; gap: 6px;
  min-height: 32px; padding: 0 10px; border: 1px solid var(--border, #d7dde4); border-radius: 8px;
  background: var(--surface, #fff); color: var(--text, #1b1f24); font: inherit; font-size: 13px; font-weight: 520;
  cursor: pointer; white-space: nowrap; text-decoration: none;
}
#images-dialog .images-btn:hover { background: var(--surface-2, #eaeef2); }
#images-dialog .images-btn.danger { color: var(--bad, #b42318); }
#images-dialog .images-btn.ghost { border-color: transparent; background: none; }
#images-dialog .images-btn.ghost:hover { background: var(--surface-2, #eaeef2); }
#images-dialog .images-btn:disabled { opacity: .45; cursor: default; }
#images-dialog :focus-visible { outline: 2px solid var(--focus, #2563eb); outline-offset: 2px; }
#images-fresh .images-list { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 8px; }
#images-fresh button {
  appearance: none; display: flex; flex-direction: column; align-items: flex-start; gap: 2px; min-width: 0;
  padding: 10px 12px; border: 1px solid var(--border, #d7dde4); border-radius: 8px; background: var(--surface, #fff);
  color: var(--text, #1b1f24); font: inherit; text-align: left; cursor: pointer;
}
#images-fresh button:hover { border-color: var(--accent, #1d6fb8); background: var(--accent-soft, #e2eefa); }
#images-fresh button[data-default] { border-color: var(--accent, #1d6fb8); }
#images-fresh .label { font-size: 15px; font-weight: 600; }
#images-fresh .sub, #images-saved .sub { font-size: 12.5px; color: var(--muted, #5b6471); overflow-wrap: anywhere; }
#images-saved ul { display: flex; flex-direction: column; gap: 6px; margin: 0; padding: 0; list-style: none; }
#images-saved li {
  display: flex; flex-wrap: wrap; align-items: center; gap: 6px 10px; padding: 8px 10px;
  border: 1px solid var(--border, #d7dde4); border-radius: 8px; min-width: 0;
}
#images-saved li[data-running] { border-color: var(--accent, #1d6fb8); }
#images-saved .what { display: flex; flex-direction: column; flex: 1 1 200px; min-width: 0; }
#images-saved .name { font-weight: 600; overflow-wrap: anywhere; }
#images-saved .acts { display: flex; flex-wrap: wrap; gap: 4px; }
#images-saved .badge { display: inline-flex; align-items: center; min-height: 32px; padding: 0 10px; border-radius: 999px;
  background: var(--accent-soft, #e2eefa); color: var(--accent, #1d6fb8); font-size: 12.5px; font-weight: 600; }
#images-saved .empty { font-size: 13.5px; color: var(--muted, #5b6471); }
#images-dialog .images-foot { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; padding-top: 10px;
  border-top: 1px solid var(--border, #d7dde4); }
#images-usage { flex: 1 1 100%; font-size: 12.5px; color: var(--muted, #5b6471); }
#images-dialog .grow { flex: 1; }
`;

  const mb = n => (n / 1048576).toFixed(1) + ' MB';
  const dateOf = t => { try { return new Date(t).toLocaleString(); } catch (e) { return String(t); } };

  function create(o) {
    const doc = o.doc || document;
    const win = doc.defaultView || window;
    const Catalog = win.PharoCatalog;
    const manifest = () => (typeof o.manifest === 'function' ? o.manifest() : o.manifest) || null;
    const current = () => (o.current && o.current()) || {};
    const running = () => (o.running && o.running()) || null;
    let launcher = false, note = null, storageNote = null;

    if (!doc.getElementById('images-style')) {
      const style = doc.createElement('style');
      style.id = 'images-style';
      style.textContent = STYLE;
      doc.head.appendChild(style);
    }
    const el = (tag, props, ...kids) => {
      const e = doc.createElement(tag);
      for (const k in props || {}) {
        if (k === 'class') e.className = props[k];
        else if (k === 'text') e.textContent = props[k];
        else if (k === 'hidden') e.hidden = !!props[k];
        else e.setAttribute(k, props[k]);
      }
      for (const kid of kids) if (kid) e.appendChild(kid);
      return e;
    };

    const dialog = el('dialog', { id: 'images-dialog', 'aria-labelledby': 'images-title' });
    const body = el('div', { class: 'images-body' });
    const noteEl = el('p', { id: 'images-note', role: 'status', hidden: true });
    const freshList = el('div', { class: 'images-list' });
    const fresh = el('section', { id: 'images-fresh', 'aria-labelledby': 'images-fresh-title' },
                     el('h3', { id: 'images-fresh-title', text: 'Start fresh' }), freshList);
    const savedList = el('ul', { 'aria-labelledby': 'images-saved-title' });
    const empty = el('p', { class: 'empty', text: 'Images you save appear here. They stay in this browser only; Download keeps a copy.' });
    const saved = el('section', { id: 'images-saved', 'aria-labelledby': 'images-saved-title' },
                     el('h3', { id: 'images-saved-title', text: 'Saved in this browser' }), savedList, empty);
    const openFile = el('button', { id: 'images-open-file', class: 'images-btn', type: 'button',
                                    title: 'Start an image of your own: a Pharo .zip, or an .image with its .changes and its .sources',
                                    text: 'Open a file…' });
    const usage = el('p', { id: 'images-usage', hidden: true });
    const persist = el('button', { id: 'images-persist', class: 'images-btn', type: 'button', hidden: true,
                                   title: 'Ask the browser not to delete the images of this site when it runs short of space',
                                   text: 'Keep my images safe from cleanup' });
    const deleteAll = el('button', { id: 'images-delete-all', class: 'images-btn danger', type: 'button',
                                     text: 'Delete all' });
    const close = el('button', { id: 'images-close', class: 'images-btn', type: 'button', text: 'Close' });
    body.append(el('h2', { id: 'images-title', text: 'Images' }), noteEl, fresh, saved,
                el('div', {}, openFile),
                el('div', { class: 'images-foot' }, usage, persist, deleteAll, el('span', { class: 'grow' }), close));
    dialog.appendChild(body);
    doc.body.appendChild(dialog);

    function showNote() {
      const text = [note, storageNote].filter(Boolean).join(' ');
      noteEl.textContent = text;
      noteEl.hidden = !text;
    }

    // the fresh images of the catalog
    function renderFresh() {
      const m = manifest();
      const all = m && Catalog ? Catalog.images(m) : [];
      freshList.textContent = '';
      fresh.hidden = !all.length;
      for (const e of all) {
        const size = Catalog.downloadSize(e);
        const sub = (e.pharoBuild ? 'build ' + e.pharoBuild + ' · ' : '') + mb(size) + ' download' +
          (e.default && all.length > 1 ? ' (default)' : '');
        const b = el('button', { type: 'button', 'data-fresh': e.id },
                     el('span', { class: 'label', text: e.label || e.id }), el('span', { class: 'sub', text: sub }));
        if (e.default) b.setAttribute('data-default', '');
        b.addEventListener('click', () => choose({ fresh: e.id }));
        freshList.appendChild(b);
      }
    }

    // the saved slots, newest first
    let listed = [], gen = 0;
    async function renderSaved() {
      const g = ++gen;
      let all = [];
      storageNote = null;
      if (!o.store) storageNote = 'This browser keeps no images here (a private window?): Start fresh and Open still work.';
      else {
        try { all = await o.store.list(); }
        catch (e) { storageNote = 'This browser keeps no images here (a private window?): Start fresh and Open still work.'; }
      }
      if (g !== gen) return;
      listed = all;
      showNote();
      // the button of a row that has the focus, given back below
      const f = doc.activeElement, row = f && savedList.contains(f) ? f.closest('li[data-slot]') : null;
      const had = row ? { slot: row.dataset.slot, act: f.dataset.act } : null;
      savedList.textContent = '';
      empty.hidden = !!all.length;
      deleteAll.hidden = !all.length;
      const here = running();
      for (const s of all) {
        const size = (s.imageSize || 0) + (s.changesSize || 0) + (s.sourcesSize || 0) + (s.treeSize || 0);
        const sub = (s.pharo ? Catalog.labelOf(s.pharo) : 'Pharo ?') + (s.savedAt ? ' · saved ' + dateOf(s.savedAt) : '') +
          ' · ' + mb(size) + (s.treeFiles > 0 ? ' · ' + s.treeFiles + ' files kept' : '');
        const li = el('li', { 'data-slot': s.slot },
                      el('span', { class: 'what' }, el('span', { class: 'name', text: s.name }),
                         el('span', { class: 'sub', text: sub })));
        const acts = el('span', { class: 'acts' });
        if (s.slot === here) {
          li.setAttribute('data-running', '');
          acts.appendChild(el('span', { class: 'badge', text: 'Running here' }));
        } else {
          const b = el('button', { class: 'images-btn', type: 'button', 'data-act': 'open', text: 'Open',
                                   'aria-label': 'Open ' + s.name });
          b.addEventListener('click', () => choose({ slot: s.slot }));
          acts.appendChild(b);
        }
        const rename = el('button', { class: 'images-btn ghost', type: 'button', 'data-act': 'rename', text: 'Rename',
                                      'aria-label': 'Rename ' + s.name });
        rename.addEventListener('click', () => renameSlot(s));
        const del = el('button', { class: 'images-btn ghost danger', type: 'button', 'data-act': 'delete', text: 'Delete',
                                   'aria-label': 'Delete ' + s.name });
        del.addEventListener('click', () => deleteSlot(s));
        acts.append(rename, del);
        if (o.otherPage) {
          const a = el('a', { class: 'images-btn ghost', 'data-act': 'other', href: o.otherPage.href({ slot: s.slot }),
                              text: o.otherPage.label, title: 'Open ' + s.name + ' in the ' + o.otherPage.label });
          acts.appendChild(a);
        }
        li.appendChild(acts);
        savedList.appendChild(li);
      }
      if (dialog.open && (had || !dialog.contains(doc.activeElement))) {
        const li = had && [...savedList.children].find(e => e.dataset.slot === had.slot);
        const again = li && [...li.querySelectorAll('[data-act]')].find(e => e.dataset.act === had.act);
        (again || (had && savedList.querySelector('button')) || firstButton()).focus();
      }
    }

    async function renderUsage() {
      if (!o.store) { usage.hidden = persist.hidden = true; return; }
      const e = await o.store.estimate().catch(() => null);
      usage.hidden = !(e && e.quota);
      if (e && e.quota) usage.textContent = 'Using ' + mb(e.usage || 0) + ' of about ' + mb(e.quota) + ' in this browser';
      const nav = win.navigator;
      const can = !!(nav && nav.storage && nav.storage.persist);
      persist.hidden = !can || await o.store.persisted().catch(() => false);
    }

    function refresh() {
      renderFresh();
      showNote();
      return Promise.all([renderSaved(), renderUsage()]).catch(e => { console.warn('images: ' + ((e && e.message) || e)); });
    }

    async function choose(choice) {
      const r = o.onOpen ? await o.onOpen(choice) : true;
      if (r !== false) closeIt();
    }
    async function renameSlot(s) {
      const name = win.prompt('Rename the image', s.name);
      if (name === null) return;
      const n = name.trim().slice(0, 200);
      if (!n || n === s.name) return;
      try { await o.store.rename(s.slot, n); }
      catch (e) { note = 'Could not rename it: ' + ((e && e.message) || e); }
      if (o.onChange) o.onChange();
      refresh();
    }
    async function deleteSlot(s) {
      if (!win.confirm("Delete '" + s.name + "' from this browser?")) return;
      try {
        if (s.slot === running() && o.onDeleteRunning) await o.onDeleteRunning(s.slot);
        else await o.store.remove(s.slot);
      } catch (e) { note = 'Could not delete it: ' + ((e && e.message) || e); }
      if (o.onChange) o.onChange();
      if (dialog.open) refresh();
    }

    function open(text, asLauncher) {
      note = text || null;
      launcher = asLauncher === undefined ? !!o.launcher : !!asLauncher;
      close.hidden = launcher;
      dialog.toggleAttribute('data-launcher', launcher);
      const shown = refresh();
      if (!dialog.open) dialog.showModal();
      if (launcher || !dialog.contains(doc.activeElement)) firstButton().focus();
      return shown;
    }
    // the default image, for Enter; else the first thing to do
    function firstButton() {
      return (launcher && freshList.querySelector('[data-default]')) || freshList.querySelector('button') ||
        savedList.querySelector('button') || openFile;
    }
    function closeIt() {
      launcher = false;
      if (dialog.open) dialog.close();
    }

    openFile.addEventListener('click', () => {
      if (!launcher) closeIt();
      if (o.onOpenFile) o.onOpenFile();
    });
    persist.addEventListener('click', async () => {
      const ok = await o.store.persist().catch(() => false);
      note = ok ? 'The browser keeps the images of this site from its cleanup now.'
                : 'The browser did not grant it: it may still delete the images of this site when it runs short of space.';
      refresh();
    });
    deleteAll.addEventListener('click', async () => {
      if (!win.confirm('Delete every image saved in this browser (' + listed.length + ')?')) return;
      try { if (o.onDeleteRunning) await o.onDeleteRunning(null); else await o.store.reset(); }
      catch (e) { note = 'Could not delete them: ' + ((e && e.message) || e); }
      if (o.onChange) o.onChange();
      if (dialog.open) refresh();
    });
    close.addEventListener('click', closeIt);
    // the launcher stays until an image is chosen: Escape is kept from it,
    // and when the browser closes it all the same, it shows again
    dialog.addEventListener('cancel', e => { if (launcher) e.preventDefault(); });
    dialog.addEventListener('close', () => {
      if (!launcher || dialog.open) return;
      dialog.showModal();
      firstButton().focus();
    });
    dialog.addEventListener('click', e => { if (e.target === dialog && !launcher) closeIt(); });
    win.addEventListener('focus', () => { if (dialog.open) refresh(); });

    return {
      open, close: closeIt, refresh,
      get isOpen() { return dialog.open; },
      get isLauncher() { return dialog.open && launcher; },
    };
  }

  return { create };
});
