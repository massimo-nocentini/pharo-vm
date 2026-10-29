// git-proxy.js - the CORS proxy of git's HTTP requests, as the user chose it
//
// UMD: the global PharoGitProxy of the pages (<script>: index.html and
// world.html), a CommonJS module in node.  The pages share it:
// the choice, where it is kept, and the Settings dialog that changes it.
//
// libgit2's smart-HTTP transport of the VM (src/emscripten/gitSupport.c)
// sends each request of git to a URL prefix, the proxy, followed by the host
// and the path of the repository: no git host lets a page of another origin
// read its answers (CORS), a proxy that adds the headers does.  The site has
// a default (manifest.gitProxyDefault, which stage.mjs takes from the build,
// WASM_GIT_PROXY; '' or absent: none), and the user may choose another, or
// none.  The choice is kept in localStorage under KEY:
//
//   absent        the site's default, whatever it is (or becomes)
//   'none'        no proxy: the requests go to the git host itself, which
//                 works only for a host that lets this page in
//   a URL         that proxy, an http: or https: URL without credentials
//                 or fragment (parse); anything else counts as none
//
// so that a build of before, which read the value as a URL, takes 'none' as
// none too.
//
// Credentials.  A push, or a fetch of a private repository, needs a user name
// and a token, which the VM's transport asks Iceberg for when the host
// answers 401, and sends in an Authorization header: through a proxy, the
// proxy can read them.  So they go only where the user chose: straight to
// the host (none), or through My proxy, never through the site's default,
// a public proxy that the user did not choose (the worker tells the
// transport, Module.gitHttpCredentials).  There the transport refuses them,
// and the page says so (describe), and offers to choose the site's proxy
// as My proxy (useForCredentials), which then holds for all git requests,
// as any proxy of the user's.  The first request that carries credentials
// through a proxy is told once per proxy (CREDENTIALS_NOTED_KEY).  Without localStorage (a private window, blocked site data) the
// choice holds for the page (a variable of this module), and so does one
// that could not be kept.  The choice is set only by the dialog, never from
// the URL: a link must not send someone's git traffic, code and
// credentials, to a proxy of its choosing.
//
//   PharoGitProxy.KEY, NOTED_KEY       'pharo-wasm.gitProxy', 'pharo-wasm.gitProxyNoted'
//   PharoGitProxy.CREDENTIALS_NOTED_KEY
//                                      'pharo-wasm.gitCredentialsNoted'
//   PharoGitProxy.parse(text)          {value: ''} (empty), {value, origin} or {error};
//                                      value is the URL as the browser writes it
//   PharoGitProxy.read()               {mode: 'default'|'none'|'custom', value: ''|url}
//   PharoGitProxy.initValue()          null (the default), '' (none) or the url: what
//                                      the worker gets in init.gitProxy and "gitProxy"
//   PharoGitProxy.write(mode, url)     {kept, error?}: keeps the choice (url for custom;
//                                      an empty one is none); kept false when this
//                                      browser could not keep it (it holds for the page)
//   PharoGitProxy.originOf(url)        its origin, or ''
//   PharoGitProxy.noted(origin, key = NOTED_KEY), markNoted(origin, key = NOTED_KEY)
//                                      whether the page told the user once that the
//                                      requests go through the public proxy origin
//                                      (NOTED_KEY keeps the origin told; without
//                                      storage, for the page), or, with
//                                      CREDENTIALS_NOTED_KEY, that credentials went
//                                      through the proxy origin
//   PharoGitProxy.useForCredentials(url)
//                                      {kept, ...}: makes url (the site's default)
//                                      My proxy, as write('custom', url) does
//   PharoGitProxy.onChange(cb)         cb(initValue()) when another page changed it
//                                      (storage events)
//   PharoGitProxy.describe(m)          what a page tells of the worker's message
//                                      git {event, host, proxy, status, credentials,
//                                      redirectedTo, proxyRefused}, or '': the first request
//                                      through the site's default, once per proxy
//                                      (then noted), the first one that carried
//                                      credentials through a proxy, once per proxy
//                                      (noted too), or why a request failed: with
//                                      credentials 'refused', that the host asked
//                                      for credentials, which would go through the
//                                      site's public proxy; with 'redirected', that a
//                                      host redirected a request with credentials to
//                                      another host, which the proxy sent them to
//   PharoGitProxy.wireDialog({defaultProxy, onSaved, refocus})
//                                      -> {open()}: the Settings dialog of the
//                                      document, which has the ids below
//
// The Settings dialog (index.html, world.html): #settings-dialog, a
// <dialog> whose form #settings-form has the radios of name git-proxy-mode
// (#git-proxy-mode-default, labelled with the site's default,
// #git-proxy-default-url, #git-proxy-mode-none and #git-proxy-mode-custom),
// the field #git-proxy of the custom one (a focus or a key in it chooses
// custom; Enter saves; after a focus alone, the field left empty, the
// choice of before comes back when the focus leaves, or at a save:
// tabbing through the dialog changes nothing), #git-proxy-state
// (data-kind default, proxy, none or bad: what the requests do then, or
// why the URL is not one), the privacy text #git-proxy-about
// (#git-proxy-about-default names the site's default), and
// #settings-cancel and #settings-save.  A save that
// changed the choice keeps it (write) and calls onSaved(initValue(), read(),
// kept); the page then gives it to its VMs.  A click on the backdrop, Cancel
// and Escape close the dialog without a change, and refocus() is called
// once it closed.

(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.PharoGitProxy = factory();
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  const KEY = 'pharo-wasm.gitProxy', NOTED_KEY = 'pharo-wasm.gitProxyNoted';
  const CREDENTIALS_NOTED_KEY = 'pharo-wasm.gitCredentialsNoted';
  const NONE = 'none';

  // The proxy as typed: {value: ''} for none, else {value, origin} for an
  // http: or https: URL without credentials or fragment, as the browser
  // writes it (https:proxy.example/git is https://proxy.example/git, which
  // the worker and the VM take; a bare origin gets its /, which the host of
  // the repository follows), else {error}
  function parse(text) {
    const s = String(text || '').trim();
    if (!s) return { value: '' };
    let u;
    try { u = new URL(s); } catch (e) { return { error: 'This is not a URL.' }; }
    if (u.protocol !== 'http:' && u.protocol !== 'https:') return { error: 'The proxy must be an http:// or https:// URL.' };
    if (u.username || u.password) return { error: 'Give the URL of the proxy without a user name or password.' };
    if (u.hash || s.indexOf('#') >= 0) return { error: 'The URL of the proxy cannot have a #fragment.' };
    return { value: u.pathname === '/' && !u.search ? u.origin + '/' : u.href, origin: u.origin };
  }

  function originOf(url) {
    try { return url ? new URL(url).origin : ''; } catch (e) { return ''; }
  }

  // localStorage, or null when this page has none (a getter that throws, in
  // a private window or with blocked site data)
  function storage() {
    try { return typeof localStorage !== 'undefined' && localStorage ? localStorage : null; } catch (e) { return null; }
  }

  // The choice of this page when the browser keeps none: null until one is
  // made here (the default then), and whether it is newer than the stored one
  let session = null, unkept = false;

  // What a stored value means: absent the default, 'none' none, a URL that
  // parse takes that proxy, anything else none
  function meaning(s) {
    if (s === null || s === undefined) return { mode: 'default', value: '' };
    if (s === NONE) return { mode: 'none', value: '' };
    const p = parse(s);
    return !p.error && p.value ? { mode: 'custom', value: p.value } : { mode: 'none', value: '' };
  }

  function read() {
    const ls = storage();
    if (!ls || unkept) return session ? Object.assign({}, session) : { mode: 'default', value: '' };
    let s;
    try { s = ls.getItem(KEY); } catch (e) { return session ? Object.assign({}, session) : { mode: 'default', value: '' }; }
    return meaning(s);
  }

  function initValue() {
    const r = read();
    return r.mode === 'default' ? null : r.mode === 'none' ? '' : r.value;
  }

  function write(mode, url) {
    let choice;
    if (mode === 'custom') {
      const p = parse(url);
      if (p.error) return { kept: false, error: p.error };
      choice = p.value ? { mode: 'custom', value: p.value } : { mode: 'none', value: '' };
    } else if (mode === 'none') choice = { mode: 'none', value: '' };
    else choice = { mode: 'default', value: '' };
    session = choice;
    const ls = storage();
    let kept = false;
    if (ls) {
      try {
        if (choice.mode === 'default') ls.removeItem(KEY);
        else ls.setItem(KEY, choice.mode === 'none' ? NONE : choice.value);
        kept = true;
      } catch (e) { /* quota, or blocked: it holds for this page */ }
    }
    unkept = !kept;
    return { kept, mode: choice.mode, value: choice.value };
  }

  // The origin of the public proxy that the page told the user of (and of
  // the proxy that it told credentials went through, under its own key)
  const notedHere = {};
  function noted(origin, key = NOTED_KEY) {
    const ls = storage();
    if (ls) {
      try { if (ls.getItem(key) === origin) return true; } catch (e) { /* the page's */ }
    }
    return notedHere[key] === origin;
  }
  function markNoted(origin, key = NOTED_KEY) {
    notedHere[key] = origin;
    const ls = storage();
    if (ls) {
      try { ls.setItem(key, origin); } catch (e) { /* the page's */ }
    }
  }

  // The site's default proxy url made My proxy, so that credentials may go
  // through it (see Credentials above)
  function useForCredentials(url) {
    return write('custom', url);
  }

  // Another page of the site changed it (a key of null: storage cleared)
  function onChange(cb) {
    if (typeof addEventListener !== 'function') return;
    addEventListener('storage', e => {
      if (e.key !== KEY && e.key !== null) return;
      unkept = false;
      cb(initValue());
    });
  }

  // What a page tells of git {event, host, proxy, status}, which the worker
  // posts at the first request of git and when one failed (proxy is the
  // origin of the proxy, or ''; status -1 or -2 without a response), or ''
  function describe(m) {
    if (!m) return '';
    const proxy = m.proxy || '';
    if (m.event === 'request') {
      if (read().mode !== 'default' || !proxy || noted(proxy)) return '';
      markNoted(proxy);
      return 'Git requests (Iceberg clones, Metacello github://) go through the public CORS proxy ' + proxy +
        ', which sees what you clone.';
    }
    if (m.event === 'credentials') {
      if (!proxy || noted(proxy, CREDENTIALS_NOTED_KEY)) return '';
      markNoted(proxy, CREDENTIALS_NOTED_KEY);
      return 'The user name and token that ' + m.host + ' asked for go through the git proxy ' + proxy +
        ', which can read them.';
    }
    if (m.event !== 'failed') return '';
    if (m.credentials === 'redirected')
      return m.host + ' redirected a request with your user name and token to ' + (m.redirectedTo || 'another host') +
        ', and the git proxy ' + proxy + ' sent them there: the request was stopped. Revoke that token if ' +
        (m.redirectedTo || 'that host') + ' is not yours.';
    if (m.credentials === 'refused')
      return m.host + ' asks for a user name and token, which this page sends only through a git proxy you chose: ' +
        "the site's default, the public proxy " + proxy + ', would see them. Choose it, or your own, then try again.';
    if (!proxy && m.status < 0) return "The browser blocked git's request to " + m.host + ' (CORS): git over HTTPS needs a proxy here.';
    if (proxy && m.status < 0) return 'The git proxy ' + proxy + ' did not answer.';
    // (a 403 of the host, a token that may not push say, has a body: the
    // transport's error says it; proxyRefused is absent before it was told)
    if (proxy && m.status === 403 && m.proxyRefused !== false) return 'The git proxy ' + proxy + ' refused the request to ' + m.host + '.';
    return '';
  }

  // ---- the Settings dialog

  function wireDialog(o) {
    const doc = o.doc || document;
    const $ = id => doc.getElementById(id);
    const dialog = $('settings-dialog'), input = $('git-proxy'), state = $('git-proxy-state');
    const radios = { default: $('git-proxy-mode-default'), none: $('git-proxy-mode-none'), custom: $('git-proxy-mode-custom') };
    const defaultProxy = () => String((o.defaultProxy && o.defaultProxy()) || '');
    const mode = () => Object.keys(radios).find(k => radios[k].checked) || 'default';
    const choose = m => { for (const k in radios) radios[k].checked = k === m; };
    // the mode that a focus of the empty field replaced by custom, until
    // something is typed or chosen
    let byFocus = null;
    function unfocus() {
      if (byFocus && !input.value.trim()) choose(byFocus);
      byFocus = null;
    }

    // the labels that name the default, and the text that names it
    function showDefault() {
      const d = defaultProxy(), origin = originOf(d) || d;
      const url = $('git-proxy-default-url'), named = $('git-proxy-default-named'), none = $('git-proxy-default-none');
      if (url) url.textContent = origin;
      if (named) named.hidden = !d;
      if (none) none.hidden = !!d;
      const about = $('git-proxy-about-default');
      if (about) about.textContent = d ? 'This site\'s default is the public proxy ' + origin + '. '
                                       : 'This site has no default proxy. ';
    }
    // what the dialog says of the choice, and of the text of the field
    function showState() {
      const m = mode();
      let kind, text, p = null;
      if (m === 'default') {
        const d = defaultProxy();
        kind = 'default';
        text = d ? 'Default: git requests go to ' + (originOf(d) || d) + '.'
                 : 'Default: no proxy, git requests go to the repository itself.';
      } else if (m === 'none') {
        kind = 'none';
        text = 'No proxy: git requests go to the repository itself.';
      } else {
        p = parse(input.value);
        kind = p.error ? 'bad' : p.value ? 'proxy' : 'none';
        text = p.error || (p.value ? 'Git requests go to ' + p.origin + '.' : 'No proxy: git requests go to the repository itself.');
      }
      input.setAttribute('aria-invalid', p && p.error ? 'true' : 'false');
      state.dataset.kind = kind;
      if (state.textContent !== text) state.textContent = text;
      return p;
    }
    function open() {
      if (dialog.open) return;
      const r = read();
      choose(r.mode);
      byFocus = null;
      input.value = r.mode === 'custom' ? r.value : '';
      showDefault();
      showState();
      dialog.showModal();
      (radios[r.mode] || radios.default).focus();
    }
    function save() {
      unfocus();
      const m = mode(), p = showState();
      if (p && p.error) { input.focus(); return; }
      dialog.close();
      const before = read();
      const after = m === 'custom' && p && p.value ? { mode: 'custom', value: p.value }
        : m === 'custom' ? { mode: 'none', value: '' } : { mode: m, value: '' };
      if (before.mode === after.mode && before.value === after.value) return;
      const w = write(after.mode, after.value);
      if (o.onSaved) o.onSaved(initValue(), read(), w.kept);
    }
    for (const k in radios) radios[k].addEventListener('change', () => {
      byFocus = null;
      if (k !== 'custom') input.value = '';
      showState();
    });
    // a focus or a key in the field chooses custom; a focus alone, until
    // the field is left
    input.addEventListener('focus', () => {
      if (radios.custom.checked) return;
      byFocus = mode();
      choose('custom');
      showState();
    });
    input.addEventListener('blur', () => { if (byFocus) { unfocus(); showState(); } });
    input.addEventListener('input', () => { byFocus = null; choose('custom'); showState(); });
    $('settings-form').addEventListener('submit', e => { e.preventDefault(); save(); });
    $('settings-cancel').addEventListener('click', () => dialog.close());
    dialog.addEventListener('close', () => { if (o.refocus) o.refocus(); });
    // a click on the backdrop, outside the form, cancels
    dialog.addEventListener('click', e => { if (e.target === dialog) dialog.close(); });
    return { open };
  }

  return { KEY, NOTED_KEY, CREDENTIALS_NOTED_KEY, parse, read, initValue, write, useForCredentials, originOf, noted, markNoted,
           onChange, describe, wireDialog };
});
