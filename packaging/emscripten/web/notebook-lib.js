// notebook-lib.js - pure helpers for the Notebook tab (notebook.js) of the Console page
//
// UMD: the global NotebookLib in the page, a CommonJS module in node
// (tests/wasm/notebook-lib.test.mjs).  No dependencies; the DOM is only
// touched through the document passed in (renderMarkdown, sanitizeMarkup,
// highlightDom) or the nodes (colorCode).
// It knows nothing of Smalltalk, which is notebook-st.js (NotebookSt): it is
// the part of CHICKEN's notebook-lib.js (emscripten/web) that does not
// depend on the language, with the same names and shapes.
//
//   renderMarkdown(src, doc, idPrefix, opts?) -> DocumentFragment
//   sanitizeMarkup(str, 'html'|'svg', idPrefix, doc?) -> DocumentFragment
//   toJson(nb) -> string                  fromJson(text, opts?) -> {title,
//                                           created, modified, pharo, cells,
//                                           warnings}
//   cleanOutput(o), cleanError(e) -> the output or ERR as saved, or null
//   limitOutputs(outputs, max) -> outputs (as Import keeps them)
//   newId() -> string                     slug(text) -> string
//   safeHref(url), safeImageSrc(url) -> the URL to use, or null
//   LIMITS                                what Import accepts
//   highlightLines(text, highlight) -> [[[kind, text]]], line by line
//   highlightDom(doc, pieces) -> DocumentFragment
//   colorCode(root, highlight, budget?) -> the budget left
//   HIGHLIGHT_MAX                         the code colorCode colors at most
//
// highlight is the language's own (NotebookSt.highlight): text -> the
// [kind, text] pieces whose texts add up to it.
//
// Untrusted markup (the kernel's text/html and image/svg+xml displays, and
// imported notebooks) is parsed inertly (DOMParser) and rebuilt element by
// element from an allowlist; nothing is ever serialized back into markup,
// and no innerHTML is used.  Markdown is built with createElement and text
// nodes only, in time linear in its source.  The page's CSP, which allows
// no inline script, is a second line of defence behind them.

(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.NotebookLib = factory();
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  const SVG_NS = 'http://www.w3.org/2000/svg';
  const HTML_NS = 'http://www.w3.org/1999/xhtml';
  const XLINK_NS = 'http://www.w3.org/1999/xlink';

  // ---- URLs

  // a link target: http, https, mailto or a fragment; returns the URL to
  // use, or null.  The URL parser drops tabs and newlines, so
  // "jav&#x09;ascript:" is caught as javascript:.
  function safeHref(u) {
    if (typeof u !== 'string') return null;
    const t = u.trim();
    if (t.startsWith('#')) return /^#[^\s"'<>`]*$/.test(t) ? t : null;
    let url;
    try { url = new URL(t); } catch (e) { return null; }
    if (url.protocol !== 'http:' && url.protocol !== 'https:' && url.protocol !== 'mailto:') return null;
    return url.href;
  }
  const IMG_DATA = /^data:image\/(png|jpeg|gif|webp)(;[a-z0-9=._-]+)*;base64,/i;
  function safeImageSrc(u) {
    if (typeof u !== 'string') return null;
    const t = u.trim(), m = IMG_DATA.exec(t);
    if (!m) return null;
    const b64 = t.slice(m[0].length).replace(/\s+/g, '');
    return /^[A-Za-z0-9+/]*={0,2}$/.test(b64) ? m[0] + b64 : null;
  }
  function slug(s) {
    return String(s).toLowerCase().trim().replace(/[^\p{L}\p{N}\s_-]/gu, '').replace(/\s+/g, '-').slice(0, 64) || 'section';
  }

  // ---- markdown (CommonMark-ish subset)

  // Time is linear in the source: no regular expression here may
  // backtrack over a long run (a page reload renders saved cells).

  function trimTail(s) {                 // trailing spaces and tabs
    let e = s.length;
    while (e > 0 && (s[e - 1] === ' ' || s[e - 1] === '\t')) e--;
    return s.slice(0, e);
  }

  const ESCAPABLE = /[!"#$%&'()*+,\-./:;<=>?@[\\\]^_`{|}~]/;

  // Linear in s: brackets are matched once (bracketPairs), a failed
  // search for an emphasis closer is not repeated (noCloser), code span
  // closers are looked up in an index of the backtick runs (tickRuns),
  // and text is flushed at each line end.
  function inline(doc, s, out, depth) {
    // out: a node to append to
    depth = depth || 0;
    if (depth > 32) { out.appendChild(doc.createTextNode(s)); return out; }
    let i = 0, text = '', pairs = null, runs = null;
    const noCloser = {};
    const flush = () => { if (text) { out.appendChild(doc.createTextNode(text)); text = ''; } };
    const n = s.length;
    while (i < n) {
      const c = s[i];
      if (c === '\\' && i + 1 < n && ESCAPABLE.test(s[i + 1])) { text += s[i + 1]; i += 2; continue; }
      if (c === '\\' && s[i + 1] === '\n') { flush(); out.appendChild(doc.createElement('br')); i += 2; continue; }
      if (c === '\n') {
        // two trailing spaces: a hard break.  A text node per line keeps
        // the trailing spaces cheap to find.
        let e = text.length;
        while (e > 0 && text[e - 1] === ' ') e--;
        const hard = text.length - e >= 2;
        text = text.slice(0, e);
        if (hard) { flush(); out.appendChild(doc.createElement('br')); }
        else { text += '\n'; flush(); }
        i++;
        while (s[i] === ' ') i++;
        continue;
      }
      if (c === '`') {
        let j = i; while (s[j] === '`') j++;
        const ticks = s.slice(i, j);
        const e = tickCloser(runs || (runs = tickRuns(s)), j - i, j);
        if (e >= 0) {
          flush();
          let code = s.slice(j, e).replace(/\n/g, ' ');
          if (code.length > 2 && code[0] === ' ' && code[code.length - 1] === ' ' && /[^ ]/.test(code))
            code = code.slice(1, -1);
          const el = doc.createElement('code');
          el.textContent = code;
          out.appendChild(el);
          i = e + ticks.length;
          continue;
        }
        text += ticks; i = j; continue;
      }
      if (c === '<') {
        const m = /^<((?:https?|mailto):[^\s<>]*)>/i.exec(s.slice(i));
        if (m) {
          const href = safeHref(m[1]);
          flush();
          if (href) out.appendChild(link(doc, href, m[1])); else out.appendChild(doc.createTextNode(m[1]));
          i += m[0].length;
          continue;
        }
      }
      if (c === 'h' && /^https?:\/\//.test(s.slice(i, i + 8)) && (i === 0 || /[\s(]/.test(s[i - 1]))) {
        const m = /^https?:\/\/[^\s<>]+/.exec(s.slice(i));
        let e = m[0].length;
        while (e > 0 && `.,;:!?'")]`.includes(m[0][e - 1])) e--;
        const u = m[0].slice(0, e), href = safeHref(u);
        if (href) { flush(); out.appendChild(link(doc, href, u)); } else text += u;
        i += u.length;
        continue;
      }
      if (c === '!' && s[i + 1] === '[') {
        const r = linkAt(s, i + 1, pairs || (pairs = bracketPairs(s, runs || (runs = tickRuns(s)))));
        if (r) {
          flush();
          const src = safeImageSrc(r.url);
          if (src) {
            const img = doc.createElement('img');
            img.src = src;
            img.alt = r.label;
            if (r.title) img.title = r.title;
            out.appendChild(img);
          } else {
            const sp = doc.createElement('span');
            sp.className = 'md-noimg';
            sp.textContent = (r.label || 'image') + ' (external image not loaded)';
            out.appendChild(sp);
          }
          i = r.end;
          continue;
        }
      }
      if (c === '[') {
        const r = linkAt(s, i, pairs || (pairs = bracketPairs(s, runs || (runs = tickRuns(s)))));
        if (r) {
          flush();
          const href = safeHref(r.url);
          if (href) {
            const a = link(doc, href, null);
            if (r.title) a.title = r.title;
            inline(doc, r.label, a, depth + 1);
            out.appendChild(a);
          } else {
            const sp = doc.createElement('span');
            inline(doc, r.label, sp, depth + 1);
            out.appendChild(sp);
          }
          i = r.end;
          continue;
        }
      }
      if (c === '*' || c === '_' || c === '~') {
        const r = emphasisAt(s, i, noCloser);
        if (r) {
          flush();
          const el = doc.createElement(r.tag);
          inline(doc, r.inner, el, depth + 1);
          out.appendChild(el);
          i = r.end;
          continue;
        }
      }
      text += c;
      i++;
    }
    flush();
    return out;
  }

  function link(doc, href, label) {
    const a = doc.createElement('a');
    a.setAttribute('href', href);
    if (!href.startsWith('#')) {
      a.setAttribute('rel', 'noopener noreferrer');
      a.setAttribute('target', '_blank');
    }
    if (label != null) a.textContent = label;
    return a;
  }

  // The runs of backticks in s, as a Map from length to their starts in
  // order.  A code span ends at the next run exactly as long as the one
  // that opens it (as in CommonMark), found by a binary search: searching
  // the rest of s for each opener would be quadratic.
  function tickRuns(s) {
    const runs = new Map();
    for (let p = s.indexOf('`'); p >= 0; ) {
      let e = p + 1; while (s[e] === '`') e++;
      let a = runs.get(e - p);
      if (!a) runs.set(e - p, a = []);
      a.push(p);
      p = s.indexOf('`', e);
    }
    return runs;
  }
  // the start of the first run of exactly LEN backticks at or after FROM, or -1
  function tickCloser(runs, len, from) {
    const a = runs.get(len);
    if (!a) return -1;
    let lo = 0, hi = a.length;
    while (lo < hi) { const m = (lo + hi) >> 1; if (a[m] < from) lo = m + 1; else hi = m; }
    return lo < a.length ? a[lo] : -1;
  }

  // "[" index -> its "]" index, outside escapes and code spans
  function bracketPairs(s, runs) {
    const pairs = new Map(), open = [];
    for (let j = 0; j < s.length; j++) {
      const c = s[j];
      if (c === '\\') { j++; continue; }
      if (c === '`') {
        let k = j; while (s[k] === '`') k++;
        const e = tickCloser(runs, k - j, k);
        j = (e >= 0 ? e + k - j : k) - 1;
        continue;
      }
      if (c === '[') open.push(j);
      else if (c === ']' && open.length) pairs.set(open.pop(), j);
    }
    return pairs;
  }

  // [label](url "title") starting at s[i] === '['
  function linkAt(s, i, pairs) {
    const j = pairs.get(i);
    if (j === undefined || s[j + 1] !== '(') return null;
    const m = /^\(\s*(<[^<>\n]*>|[^\s()]*(?:\([^\s()]*\)[^\s()]*)*)(?:\s+("[^"]*"|'[^']*'))?\s*\)/.exec(s.slice(j + 1));
    if (!m) return null;
    let url = m[1];
    if (url.startsWith('<')) url = url.slice(1, -1);
    return { label: s.slice(i + 1, j), url, title: m[2] ? m[2].slice(1, -1) : '', end: j + 1 + m[0].length };
  }

  // noCloser[d]: a search for a closer d from there on found none
  function emphasisAt(s, i, noCloser) {
    const c = s[i];
    const dbl = s[i + 1] === c;
    if (c === '~' && !dbl) return null;
    const tryDelim = (d, tag) => {
      const after = s[i + d.length];
      if (!after || /\s/.test(after)) return null;
      // _ inside words is not emphasis
      if (c === '_' && i > 0 && /[\p{L}\p{N}]/u.test(s[i - 1])) return null;
      let k = i + d.length;
      if (noCloser[d] !== undefined && k >= noCloser[d]) return null;
      const from = k;
      while ((k = s.indexOf(d, k + 1)) > 0) {
        if (/\s/.test(s[k - 1]) || s[k - 1] === '\\') continue;
        if (d.length === 1 && s[k + 1] === c) {
          // "**" inside "*...*": skip the pair
          if (s[k - 1] !== c) { k++; continue; }
        }
        if (c === '_' && /[\p{L}\p{N}]/u.test(s[k + d.length] || '')) continue;
        return { tag, inner: s.slice(i + d.length, k), end: k + d.length };
      }
      noCloser[d] = from;
      return null;
    };
    if (c === '~') return tryDelim('~~', 'del');
    if (dbl) {
      const r = tryDelim(c + c, 'strong');
      if (r) return r;
    }
    return tryDelim(c, 'em');
  }

  // a fence line: [line, indent, fence, language] or null
  function fenceOf(l) {
    const m = /^( {0,3})(`{3,}|~{3,})(.*)$/.exec(l);
    return m && !m[3].includes('`') ? [l, m[1], m[2], /^\s*(\S*)/.exec(m[3])[1]] : null;
  }
  // an ATX heading: [line, hashes, text] or null
  function headingOf(l) {
    let i = 0;
    while (i < 3 && l[i] === ' ') i++;
    let j = i;
    while (l[j] === '#') j++;
    if (j === i || j - i > 6 || (j < l.length && l[j] !== ' ' && l[j] !== '\t')) return null;
    let t = trimTail(l.slice(j)).replace(/^[ \t]+/, '');
    let e = t.length;
    while (e > 0 && t[e - 1] === '#') e--;    // a closing sequence
    if (e === 0) t = '';
    else if (e < t.length && (t[e - 1] === ' ' || t[e - 1] === '\t')) t = trimTail(t.slice(0, e));
    return [l, l.slice(i, j), t];
  }
  const RE_HR = /^ {0,3}([-*_])(?:[ \t]*\1){2,}[ \t]*$/;
  const RE_LIST = /^( {0,3})([-*+]|\d{1,9}[.)])([ \t]+|$)(.*)$/;
  const RE_QUOTE = /^ {0,3}> ?/;
  const RE_TABLE_DELIM = /^\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?$/;     // on a trimmed line
  const isDelim = l => l.includes('-') && RE_TABLE_DELIM.test(l.trim());
  const blank = l => /^\s*$/.test(l);

  function splitRow(l) {
    let t = l.trim();
    if (t.startsWith('|')) t = t.slice(1);
    if (t.endsWith('|') && !t.endsWith('\\|')) t = t.slice(0, -1);
    const cells = [];
    let cur = '';
    for (let i = 0; i < t.length; i++) {
      if (t[i] === '\\' && t[i + 1] === '|') { cur += '|'; i++; }
      else if (t[i] === '|') { cells.push(cur.trim()); cur = ''; }
      else cur += t[i];
    }
    cells.push(cur.trim());
    return cells;
  }

  function startsBlock(l) {
    return headingOf(l) || fenceOf(l) || RE_HR.test(l) || RE_QUOTE.test(l) ||
      /^ {0,3}([-*+]|1[.)])[ \t]+\S/.test(l);
  }

  function blocks(doc, lines, out, prefix, ids, depth, next, fence) {
    let i = 0;
    const n = lines.length;
    while (i < n) {
      const l = lines[i];
      if (blank(l)) { i++; continue; }
      let m;
      if ((m = fenceOf(l))) {
        const run = m[2], ind = m[1].length;
        const body = [];
        i++;
        while (i < n && !new RegExp('^ {0,3}' + run[0] + '{' + run.length + ',}\\s*$').test(lines[i])) {
          body.push(lines[i].replace(new RegExp('^ {0,' + ind + '}'), ''));
          i++;
        }
        i++;
        const pre = doc.createElement('pre'), code = doc.createElement('code');
        if (m[3]) code.setAttribute('data-lang', m[3].slice(0, 32));
        code.textContent = body.join('\n');
        // the language of the notebook (opts.fence), for colorCode()
        if (fence && fence.test(m[3])) code.setAttribute('data-syn', '');
        pre.appendChild(code);
        out.appendChild(pre);
        continue;
      }
      if ((m = headingOf(l))) {
        const h = doc.createElement('h' + m[1].length);
        const txt = m[2] || '';
        // the first free one of id, id-2, id-3, ...; the search goes on
        // from where the last one for this id in this source ended, so a
        // source of many "# x" is linear (next: base -> the suffix to try)
        const base = prefix + slug(txt);
        let id = base;
        if (ids.has(id)) {
          let k = next.get(base) || 2;
          while (ids.has(base + '-' + k)) k++;
          id = base + '-' + k;
          next.set(base, k + 1);
        }
        ids.add(id);
        h.id = id;
        inline(doc, txt, h);
        out.appendChild(h);
        i++;
        continue;
      }
      if (RE_HR.test(l)) { out.appendChild(doc.createElement('hr')); i++; continue; }
      if (RE_QUOTE.test(l)) {
        const body = [];
        while (i < n && (RE_QUOTE.test(lines[i]) || (!blank(lines[i]) && body.length && !startsBlock(lines[i])))) {
          body.push(lines[i].replace(RE_QUOTE, ''));
          i++;
        }
        const bq = doc.createElement('blockquote');
        if (depth < 16) blocks(doc, body, bq, prefix, ids, depth + 1, next, fence);
        out.appendChild(bq);
        continue;
      }
      if ((m = RE_LIST.exec(l))) {
        const ordered = /\d/.test(m[2]);
        const list = doc.createElement(ordered ? 'ol' : 'ul');
        if (ordered && parseInt(m[2], 10) !== 1) list.setAttribute('start', String(parseInt(m[2], 10)));
        const baseInd = m[1].length;
        while (i < n) {
          const mm = RE_LIST.exec(lines[i]);
          if (!mm || mm[1].length !== baseInd || /\d/.test(mm[2]) !== ordered) break;
          const contentInd = mm[1].length + mm[2].length + Math.max(1, Math.min(4, mm[3].length));
          const body = [mm[4]];
          i++;
          let sawBlank = false;
          while (i < n) {
            const x = lines[i];
            if (blank(x)) { sawBlank = true; body.push(''); i++; continue; }
            const ind = x.length - x.trimStart().length;
            if (ind >= contentInd || (ind > baseInd && RE_LIST.test(x))) {
              body.push(x.slice(Math.min(ind, contentInd)));
              sawBlank = false;
              i++;
              continue;
            }
            if (!sawBlank && !startsBlock(x) && !RE_LIST.test(x)) { body.push(x.trim()); i++; continue; }
            break;
          }
          while (body.length && blank(body[body.length - 1])) body.pop();
          const li = doc.createElement('li');
          const tight = !body.some(blank);
          if (depth < 16) {
            if (tight && !body.slice(1).some(x => startsBlock(x) || RE_LIST.test(x) || /^ {4}/.test(x))) inline(doc, body.join('\n'), li);
            else {
              blocks(doc, body, li, prefix, ids, depth + 1, next, fence);
              // tight lists: unwrap single paragraphs
              if (tight && li.firstChild && li.firstChild.nodeName === 'P') {
                const p = li.firstChild;
                while (p.firstChild) li.insertBefore(p.firstChild, p);
                li.removeChild(p);
              }
            }
          }
          list.appendChild(li);
          if (i < n && blank(lines[i - 1] || 'x') && !RE_LIST.test(lines[i])) break;
        }
        out.appendChild(list);
        continue;
      }
      if (/^ {4}/.test(l) || /^\t/.test(l)) {
        const body = [];
        while (i < n && (/^ {4}|^\t/.test(lines[i]) || (blank(lines[i]) && i + 1 < n && /^ {4}|^\t/.test(lines[i + 1])))) {
          body.push(lines[i].replace(/^( {4}|\t)/, ''));
          i++;
        }
        const pre = doc.createElement('pre'), code = doc.createElement('code');
        code.textContent = body.join('\n');
        pre.appendChild(code);
        out.appendChild(pre);
        continue;
      }
      if (l.includes('|') && i + 1 < n && isDelim(lines[i + 1])) {
        const head = splitRow(l);
        const aligns = splitRow(lines[i + 1]).map(d => /^:-+:$/.test(d) ? 'center' : /-:$/.test(d) ? 'right' : /^:/.test(d) ? 'left' : '');
        if (aligns.length === head.length) {
          i += 2;
          const table = doc.createElement('table'), thead = doc.createElement('thead'), tr = doc.createElement('tr');
          head.forEach((h, k) => {
            const th = doc.createElement('th');
            if (aligns[k]) th.style.textAlign = aligns[k];
            inline(doc, h, th);
            tr.appendChild(th);
          });
          thead.appendChild(tr);
          table.appendChild(thead);
          const tbody = doc.createElement('tbody');
          while (i < n && !blank(lines[i]) && lines[i].includes('|')) {
            const row = splitRow(lines[i]);
            const r = doc.createElement('tr');
            // a short row gets one cell for the missing ones: the DOM
            // stays linear in the source (N columns, N rows of "|")
            const m = Math.min(row.length, head.length);
            for (let k = 0; k < m; k++) {
              const td = doc.createElement('td');
              if (aligns[k]) td.style.textAlign = aligns[k];
              inline(doc, row[k], td);
              r.appendChild(td);
            }
            if (m < head.length) {
              const td = doc.createElement('td');
              td.setAttribute('colspan', String(head.length - m));
              r.appendChild(td);
            }
            tbody.appendChild(r);
            i++;
          }
          table.appendChild(tbody);
          const wrap = doc.createElement('div');
          wrap.className = 'md-table';
          wrap.appendChild(table);
          out.appendChild(wrap);
          continue;
        }
      }
      // paragraph
      const para = [];
      while (i < n && !blank(lines[i]) && !(para.length && startsBlock(lines[i])) &&
             !(para.length && lines[i].includes('|') && i + 1 < n && isDelim(lines[i + 1]))) {
        para.push(lines[i]);
        i++;
      }
      if (!para.length) { para.push(lines[i]); i++; }
      const p = doc.createElement('p');
      inline(doc, trimTail(para.join('\n').replace(/^ +/, '')), p);
      out.appendChild(p);
    }
  }

  // opts: {ids, fence, highlight}, or ids alone.  ids: a Set of the heading
  // ids taken (by other cells), to which the new ones are added.  fence: a
  // RegExp of the languages of the code blocks to color ('' when a fence
  // names none: NotebookSt.FENCE), which get a data-syn attribute; with
  // highlight they are colored at once (colorCode), else they are left for
  // colorCode, as a text cell's are until it comes into view.
  function renderMarkdown(src, doc, idPrefix, opts) {
    const o = opts instanceof Set ? { ids: opts } : opts || {};
    const frag = doc.createDocumentFragment();
    const prefix = idPrefix || '';
    const lines = String(src == null ? '' : src).replace(/\r\n?/g, '\n').split('\n');
    const fence = o.fence instanceof RegExp ? o.fence : null;
    blocks(doc, lines, frag, prefix, o.ids instanceof Set ? o.ids : new Set(), 0, new Map(), fence);
    // "#slug" links go to the headings, whose ids carry the prefix
    for (const a of frag.querySelectorAll('a[href^="#"]'))
      a.setAttribute('href', '#' + prefix + a.getAttribute('href').slice(1));
    if (fence && typeof o.highlight === 'function') colorCode(frag, o.highlight);
    return frag;
  }

  // ---- syntax highlighting

  // the code that colorCode() colors per call, by default; the rest is left
  // plain (the editor has its own limit per cell)
  const HIGHLIGHT_MAX = 32 * 1024;

  // The pieces of highlight(text) line by line, without the line breaks:
  // a line per \n, and pieces that end at the line breaks (a comment of
  // three lines is a piece on each).  pieces in place of text are taken as
  // they are
  function highlightLines(text, highlight) {
    const lines = [[]];
    for (const [k, s] of Array.isArray(text) ? text : highlight(String(text))) {
      const parts = s.split('\n');
      for (let j = 0; j < parts.length; j++) {
        if (j) lines.push([]);
        if (parts[j]) lines[lines.length - 1].push([k, parts[j]]);
      }
    }
    return lines;
  }

  // The pieces as DOM: a span of class syn-KIND for each piece of a kind,
  // a text node for the plain ones; createElement and textContent only
  function highlightDom(doc, pieces) {
    const frag = doc.createDocumentFragment();
    for (const [k, s] of pieces) {
      if (!k) { frag.appendChild(doc.createTextNode(s)); continue; }
      const e = doc.createElement('span');
      e.className = 'syn-' + k;
      e.textContent = s;
      frag.appendChild(e);
    }
    return frag;
  }

  // Colors the code blocks that renderMarkdown marked (code[data-syn]) under
  // root, with highlight, each at most once, up to BUDGET characters in all
  // (HIGHLIGHT_MAX by default): a block larger than what is left stays
  // plain.  Answers the budget left
  function colorCode(root, highlight, budget) {
    let left = budget === undefined ? HIGHLIGHT_MAX : budget;
    const doc = root.ownerDocument || root;
    for (const code of root.querySelectorAll('code[data-syn]')) {
      code.removeAttribute('data-syn');
      const text = code.textContent;
      if (text.length > left) continue;
      left -= text.length;
      code.replaceChildren(highlightDom(doc, highlight(text)));
    }
    return left;
  }

  // ---- markup sanitizer

  const HTML_OK = new Set(('a abbr b bdi bdo blockquote br caption cite code col colgroup dd del details dfn div dl dt em ' +
    'figcaption figure h1 h2 h3 h4 h5 h6 hr i img ins kbd li mark meter ol p pre progress q rp rt ruby s samp small ' +
    'span strong sub summary sup table tbody td tfoot th thead time tr u ul var wbr section article header footer ' +
    'aside nav main hgroup address center font big tt strike').split(' '));
  // dropped with everything inside
  const HTML_DROP = new Set(('script style template noscript iframe frame frameset object embed applet form input button ' +
    'select textarea option optgroup datalist output label fieldset legend dialog math link meta base title head ' +
    'audio video source track picture canvas map area portal slot noembed noframes xmp plaintext marquee svg:image').split(' '));
  const SVG_OK = new Set(('svg g defs symbol use path rect circle ellipse line polyline polygon text tspan textPath ' +
    'title desc linearGradient radialGradient stop clipPath mask pattern marker filter a switch ' +
    'feBlend feColorMatrix feComponentTransfer feComposite feDiffuseLighting feDisplacementMap feDistantLight ' +
    'feDropShadow feFlood feFuncA feFuncB feFuncG feFuncR feGaussianBlur feMerge feMergeNode feMorphology feOffset ' +
    'fePointLight feSpecularLighting feSpotLight feTile feTurbulence').split(' '));
  const HTML_ATTRS = {
    '*': ['title', 'lang', 'dir', 'id', 'style', 'role', 'align', 'width', 'height', 'hidden'],
    a: ['href'], img: ['src', 'alt'], td: ['colspan', 'rowspan', 'valign'], th: ['colspan', 'rowspan', 'scope', 'valign'],
    col: ['span'], colgroup: ['span'], ol: ['start', 'reversed', 'type'], ul: ['type'], li: ['value'],
    progress: ['value', 'max'], meter: ['value', 'min', 'max', 'low', 'high', 'optimum'], time: ['datetime'],
    details: ['open'], table: ['border', 'cellpadding', 'cellspacing'], font: ['color', 'size'], abbr: [], dfn: [],
  };
  const SVG_ATTRS = new Set(('id style x y x1 y1 x2 y2 cx cy r rx ry fx fy fr width height d points transform viewBox ' +
    'preserveAspectRatio fill fill-opacity fill-rule stroke stroke-width stroke-linecap stroke-linejoin ' +
    'stroke-dasharray stroke-dashoffset stroke-opacity stroke-miterlimit opacity color font-family font-size ' +
    'font-weight font-style font-variant text-anchor dominant-baseline alignment-baseline baseline-shift ' +
    'letter-spacing word-spacing text-decoration writing-mode dx dy rotate textLength lengthAdjust startOffset ' +
    'offset stop-color stop-opacity gradientUnits gradientTransform spreadMethod patternUnits patternContentUnits ' +
    'patternTransform clipPathUnits maskUnits maskContentUnits clip-path clip-rule mask marker-start marker-mid ' +
    'marker-end markerWidth markerHeight markerUnits refX refY orient filter filterUnits primitiveUnits in in2 ' +
    'result stdDeviation mode operator k1 k2 k3 k4 type tableValues slope intercept amplitude exponent ' +
    'baseFrequency numOctaves seed stitchTiles scale xChannelSelector yChannelSelector flood-color flood-opacity ' +
    'lighting-color radius surfaceScale diffuseConstant specularConstant specularExponent kernelUnitLength azimuth ' +
    'elevation pointsAtX pointsAtY pointsAtZ limitingConeAngle z display visibility overflow vector-effect ' +
    'shape-rendering text-rendering image-rendering paint-order pathLength version role systemLanguage ' +
    'aria-label aria-hidden aria-labelledby aria-describedby tabindex href').split(' '));
  // image-set(), src(), image() and cross-fade() load images without url()
  const BAD_CSS = /url\s*\(|image-set|src\s*\(|image\s*\(|cross-fade|element\s*\(|expression|@import|-moz-binding|behavior|javascript:|\\|<|>/i;
  // The only CSS functions an SVG attribute may use (most of them are
  // presentation attributes, which are CSS values), and no escapes.
  const CSS_FUNCS = new Set(('url rgb rgba hsl hsla hwb lab lch oklab oklch color color-mix light-dark ' +
    'calc min max clamp round mod rem abs sign sin cos tan asin acos atan atan2 pow sqrt hypot log exp ' +
    'matrix matrix3d translate translatex translatey translatez translate3d scale scalex scaley scalez scale3d ' +
    'rotate rotatex rotatey rotatez rotate3d skew skewx skewy perspective ' +
    'blur brightness contrast drop-shadow grayscale hue-rotate invert opacity saturate sepia ' +
    'inset circle ellipse polygon path rect xywh linear-gradient radial-gradient conic-gradient ' +
    'repeating-linear-gradient repeating-radial-gradient repeating-conic-gradient').split(' '));
  function cssValueOk(v) {
    if (v.includes('\\')) return false;
    for (const m of v.matchAll(/([a-zA-Z_-][\w-]*)\s*\(/g)) if (!CSS_FUNCS.has(m[1].toLowerCase())) return false;
    return true;
  }

  function cleanStyle(v, prefix) {
    const keep = [];
    for (const decl of String(v).split(';')) {
      const d = decl.trim();
      if (!d || !/^[-a-zA-Z]+\s*:/.test(d)) continue;
      const fragOnly = d.replace(/url\(\s*(['"]?)#([\w.:-]+)\1\s*\)/g, (_, q, id) => 'URLFRAG' + id + 'URLFRAG');
      if (BAD_CSS.test(fragOnly)) continue;
      keep.push(fragOnly.replace(/URLFRAG([\w.:-]+)URLFRAG/g, (_, id) => 'url(#' + prefix + id + ')'));
    }
    return keep.join('; ');
  }
  const fixId = (prefix, id) => prefix + String(id).replace(/[^\w.:-]/g, '_').slice(0, 64);
  // the attributes that name ids: the markup's own, never the page's
  const ID_REFS = new Set(('aria-labelledby aria-describedby aria-owns aria-controls aria-activedescendant ' +
    'aria-details aria-flowto aria-errormessage').split(' '));

  function cleanAttr(el, name, value, svg, prefix) {
    // returns the value to set, or null to drop it
    if (/^on/i.test(name) || name === 'srcdoc' || name === 'formaction' || name === 'class' ||
        name.includes(':') && name !== 'xlink:href') return null;
    const v = String(value);
    if (name === 'style') { const s = cleanStyle(v, prefix); return s || null; }
    if (name === 'id') return fixId(prefix, v);
    // focusable or not, but never ahead of the page in the Tab order
    if (name === 'tabindex') return Number(v) < 0 ? '-1' : '0';
    if (ID_REFS.has(name))
      return v.split(/\s+/).filter(Boolean).map(x => fixId(prefix, x)).join(' ');
    if (name === 'href' || name === 'xlink:href') {
      const tag = el.localName;
      const h = safeHref(v);
      if (!h) return null;
      if (h.startsWith('#')) return '#' + fixId(prefix, h.slice(1));
      return tag === 'a' ? h : null;      // use, textPath, gradients: fragments only
    }
    if (name === 'src') return el.localName === 'img' && !svg ? safeImageSrc(v) : null;
    if (svg && name !== 'role' && !/^aria-/.test(name) && !cssValueOk(v)) return null;
    if (/url\s*\(/i.test(v)) {
      const m = /^\s*url\(\s*(['"]?)#([\w.:-]+)\1\s*\)\s*(.*)$/.exec(v);
      if (!m || /url\s*\(/i.test(m[3])) return null;
      return 'url(#' + fixId(prefix, m[2]) + ')' + (m[3] ? ' ' + m[3] : '');
    }
    if (/javascript:|data:|vbscript:/i.test(v.replace(/[\s\u0000-\u001f]/g, ''))) return null;
    return v;
  }

  function rebuild(doc, src, parent, prefix, depth) {
    if (depth > 64) return;
    for (let n = src.firstChild; n; n = n.nextSibling) {
      if (n.nodeType === 3 || n.nodeType === 4) {     // text, CDATA
        parent.appendChild(doc.createTextNode(n.nodeValue));
        continue;
      }
      if (n.nodeType !== 1) continue;
      const ns = n.namespaceURI;
      const svg = ns === SVG_NS;
      // in SVG only SVG (there is no foreignObject): an HTML element in
      // it, which only XML can make, shows nothing and would be parsed
      // out of the SVG if the markup were read again
      if (!svg && parent.namespaceURI === SVG_NS) continue;
      const tag = svg ? n.localName : String(n.localName).toLowerCase();
      let ok;
      if (svg) ok = SVG_OK.has(tag);
      else if (ns === HTML_NS || ns === null) ok = HTML_OK.has(tag) && !HTML_DROP.has(tag);
      else ok = false;
      if (!ok) {
        // unknown harmless wrappers keep their (sanitized) content
        const drop = svg || ns !== HTML_NS || HTML_DROP.has(tag) || /^(script|style|iframe|object|embed)$/i.test(tag);
        if (!drop) rebuild(doc, n, parent, prefix, depth + 1);
        continue;
      }
      // (tidyHtml writes a <bdi> as a <span data-nb-bdi>, and dir as data-nb-dir)
      const bdi = !svg && (tag === 'bdi' || tag === 'span' && n.hasAttribute('data-nb-bdi'));
      const el = svg ? doc.createElementNS(SVG_NS, tag) : doc.createElement(bdi ? 'bdi' : tag);
      const allowed = svg ? null : new Set([...(HTML_ATTRS['*']), ...(HTML_ATTRS[tag] || [])]);
      const attrs = [];
      for (const a of [...n.attributes]) {
        let name = a.namespaceURI === XLINK_NS && a.localName === 'href' ? 'xlink:href' : a.name;
        let lname = svg ? name : name.toLowerCase();
        if (!svg && lname === 'data-nb-dir') lname = 'dir';
        const isAria = /^aria-[a-z]+$/.test(lname);
        if (svg ? !(SVG_ATTRS.has(lname) || isAria || lname === 'xlink:href') : !(allowed.has(lname) || isAria)) continue;
        const v = cleanAttr(n, lname, a.value, svg, prefix);
        if (v == null) continue;
        attrs.push([lname === 'xlink:href' ? 'href' : lname, v]);
      }
      if (tag === 'img' && !attrs.some(([a]) => a === 'src')) {
        const alt = n.getAttribute('alt');
        if (alt) parent.appendChild(doc.createTextNode(alt));
        continue;
      }
      // The children first, then the attributes: Chromium works out the
      // direction of a dir=auto element (and of a <bdi>) again at each child
      // added to it, from all its text (or from a fragment of them: a <bdi>
      // of 10000 children took 2 s).  A <bdi> has one child, a <span> of them.
      if (bdi) {
        const kids = doc.createElement('span');
        rebuild(doc, n, kids, prefix, depth + 1);
        el.appendChild(kids);
      } else rebuild(doc, n, el, prefix, depth + 1);
      for (const [a, v] of attrs) {
        if (a === 'dir' && el.hasAttribute(a)) continue;      // (dir and data-nb-dir: the first)
        try { el.setAttribute(a, v); } catch (e) { /* invalid name */ }
      }
      if (tag === 'a' && el.hasAttribute('href') && !el.getAttribute('href').startsWith('#')) {
        el.setAttribute('rel', 'noopener noreferrer');
        el.setAttribute('target', '_blank');
      }
      parent.appendChild(el);
    }
  }

  // <use> copies its target, which may hold more <use>: ten uses of a
  // group of ten uses of ... render 10^6 elements from a kilobyte.  The
  // rendered size of each <use> (its target's elements, uses expanded)
  // is counted, and a <use> that would take the total for the markup
  // past BUDGET is dropped, as is one in a cycle or in a chain deeper
  // than USE_CHAIN.  (Ids are prefixed per output, so a <use> only
  // finds its target in the same markup.)
  const USE_CHAIN = 16;
  function limitUses(root, budget) {
    const uses = [...root.querySelectorAll('use')];
    if (!uses.length) return;
    const byId = new Map();
    for (const e of root.querySelectorAll('[id]')) if (!byId.has(e.id)) byId.set(e.id, e);
    const target = u => {
      const h = u.getAttribute('href');
      return h && h[0] === '#' ? byId.get(h.slice(1)) : undefined;
    };
    const memo = new Map();
    // EL's rendered elements, uses expanded; Infinity in a cycle
    function weight(el, chain) {
      if (memo.has(el)) return memo.get(el);
      if (chain > USE_CHAIN) return Infinity;
      memo.set(el, Infinity);
      let w = 1;
      if (el.localName === 'use') { const t = target(el); if (t) w += weight(t, chain + 1); }
      for (let c = el.firstElementChild; c && w <= budget; c = c.nextElementSibling) w += weight(c, chain);
      if (w > budget) w = Infinity;
      memo.set(el, w);
      return w;
    }
    let total = 0;
    for (const u of uses) {
      if (!root.contains(u)) continue;    // inside a dropped one
      const t = target(u);
      const w = t ? weight(t, 1) : 0;
      if (total + w > budget) u.remove(); else total += w;
    }
  }

  // A filter costs about its primitives times the area it covers, for
  // every element it applies to, at every paint: twenty 800x800 rects
  // sharing a filter of five blurs and dilations take minutes to paint,
  // from 1.7 KB.  Each element with a filter (SVG or CSS, or a shadow)
  // is charged its weight (its primitives, kernel sizes counting) times
  // the copies of it that are rendered (uses counting), and loses it
  // when the total for the markup would go past FILTER_BUDGET (a blur
  // weighs 4), so that what is kept costs about as much as 16 blurs of
  // the whole output, at most (in headless Chromium and Firefox, which
  // paint in software, 0.6 s per paint of an 800x800 SVG, 1.5 s of a
  // 1200x800 HTML element).  So does a filter inside a mask, pattern,
  // marker or clip path, painted once per user (or vertex).
  const FILTER_BUDGET = 64;
  const FE_WEIGHT = { feGaussianBlur: 4, feDropShadow: 5, feDiffuseLighting: 3, feSpecularLighting: 3,
                      feDisplacementMap: 2, feMerge: 0, feMergeNode: 1, feDistantLight: 0, fePointLight: 0,
                      feSpotLight: 0, feFuncA: 0, feFuncB: 0, feFuncG: 0, feFuncR: 0 };
  const FILTER_PROPS = /^\s*(filter|-webkit-filter|backdrop-filter|-webkit-backdrop-filter|box-shadow|text-shadow)\s*:/i;
  const NOT_RENDERED = new Set(['defs', 'symbol', 'clipPath', 'mask', 'pattern', 'marker', 'linearGradient',
                                'radialGradient', 'filter', 'title', 'desc']);
  const REPAINTED = new Set(['clipPath', 'mask', 'pattern', 'marker']);
  function limitFilters(root) {
    const els = [...root.querySelectorAll('*')];
    const byId = new Map(), usesOf = new Map();
    for (const e of els) if (e.id && !byId.has(e.id)) byId.set(e.id, e);
    for (const u of els) {
      if (u.localName !== 'use') continue;
      const h = u.getAttribute('href'), t = h && h[0] === '#' && byId.get(h.slice(1));
      if (t) { if (!usesOf.has(t)) usesOf.set(t, []); usesOf.get(t).push(u); }
    }
    const num = v => { const n = Number(v); return isFinite(n) ? Math.abs(n) : 0; };
    const fweights = new Map();
    function filterWeight(f) {
      if (!f || f.localName !== 'filter') return 0;
      if (fweights.has(f)) return fweights.get(f);
      fweights.set(f, 0);                 // an href cycle
      let w = 0, prims = 0;
      for (const p of f.querySelectorAll('*')) {
        const n = p.localName;
        prims++;
        if (n === 'feMorphology')
          w += 1 + Math.max(0, ...String(p.getAttribute('radius') || '').split(/[\s,]+/).map(num)) / 4;
        else if (n === 'feTurbulence') w += 2 * Math.max(1, num(p.getAttribute('numOctaves') || 1));
        else w += n in FE_WEIGHT ? FE_WEIGHT[n] : 1;
      }
      // a filter without primitives takes those of the one it links to
      const h = f.getAttribute('href');
      if (!prims && h && h[0] === '#') w = filterWeight(byId.get(h.slice(1)));
      fweights.set(f, w);
      return w;
    }
    // what a filter value costs: url(#f) its filter, functions 1 to 5
    function chainWeight(v) {
      let w = 0;
      for (const m of String(v).matchAll(/([a-zA-Z-]+)\s*\(\s*(?:['"]?#([\w.:-]+))?/g)) {
        const f = m[1].toLowerCase();
        w += f === 'url' ? filterWeight(m[2] && byId.get(m[2])) : f === 'blur' ? 4 : f === 'drop-shadow' ? 5 : 1;
      }
      return w;
    }
    function weight(el) {
      let w = el.hasAttribute('filter') ? chainWeight(el.getAttribute('filter')) : 0;
      for (const d of String(el.getAttribute('style') || '').split(';')) {
        const m = FILTER_PROPS.exec(d);
        if (!m) continue;
        const v = d.slice(m[0].length);
        if (/^\s*none\s*$/i.test(v)) continue;
        // a shadow costs a blur each (a comma outside parentheses each)
        w += /shadow$/i.test(m[1]) ? 4 * (v.replace(/\([^)]*\)/g, '').split(',').length) : chainWeight(v);
      }
      return w;
    }
    // the copies of EL rendered: where it is in the tree, and uses of it
    const memo = new Map();
    function renders(el) {
      if (memo.has(el)) return memo.get(el);
      memo.set(el, 0);                    // limitUses dropped cycles
      const p = el.parentElement;
      let n = !p || p === root ? 1 : NOT_RENDERED.has(p.localName) ? viaUses(p) : renders(p);
      n += viaUses(el);
      memo.set(el, n);
      return n;
    }
    function viaUses(el) {
      let n = 0;
      for (const u of usesOf.get(el) || []) n += renders(u);
      return n;
    }
    function repainted(el) {
      for (let a = el.parentElement; a && a !== root; a = a.parentElement) if (REPAINTED.has(a.localName)) return true;
      return false;
    }
    let total = 0;
    for (const el of els) {
      if (!el.hasAttribute('filter') && !el.hasAttribute('style')) continue;
      const w = weight(el);
      if (!w) continue;
      const cost = repainted(el) ? Infinity : w * renders(el);
      if (total + cost <= FILTER_BUDGET) { total += cost; continue; }
      el.removeAttribute('filter');
      if (el.hasAttribute('style')) {
        const kept = el.getAttribute('style').split(';').filter(d => !FILTER_PROPS.test(d)).join(';');
        if (kept.trim()) el.setAttribute('style', kept); else el.removeAttribute('style');
      }
    }
  }

  // The browsers' HTML parsers take time quadratic in the depth of the
  // open elements (100000 unclosed <div> take half a minute), and which
  // start tags stay open depends on their context: <rt> outside a <ruby>,
  // <li> after a <dd>, <div/>, <div></x>, a void element inside <svg> ...
  // Chromium's also takes time quadratic in the children of some elements,
  // however shallow: of an <option> or a <fieldset> (100 KB of 't<br>' in
  // one took 9 s), a <canvas>, a <bdi> or a dir=auto element.  So untrusted
  // markup never reaches the parser as it is.  tidyHtml reads it with the
  // tokenizer of the HTML standard and a model of its tree builder (implied
  // end tags, scopes, formatting elements reopened, the form pointer, foreign
  // content), and writes it again: every element it opens is closed by an
  // end tag of its own where the model closes it, void and self-closed
  // elements are closed at once, attributes are double-quoted, a < in text
  // or in a value is &lt;, comments, doctypes, <base> and the raw-text
  // elements are left out, and CDATA is made text.  What rebuild would not
  // show is not written either: an element that it drops with its content
  // (HTML_DROP, MathML, an SVG element it does not know, HTML inside SVG),
  // with that content, and what nests deeper than the 65 levels it shows.
  // Only the attributes that it may keep are written, dir as data-nb-dir
  // and a <bdi> as a <span data-nb-bdi>, and an element that it unwraps
  // with none, as <nb-w> when it is an ordinary one to the parser.  The
  // parser then sees only elements nested as written, at most 65 deep, and
  // no tag but those (and empty comments): where the model errs, the page
  // shows the markup a little differently, never more deeply nested.
  // Still, tidyHtml refuses markup that nests more than MAX_NEST deep, whose
  // sum over its tokens of the depth of the model at each passes MAX_WORK
  // (which bounds its own work), or which it would write more than six times
  // as long; it takes time linear in the markup, and so does the parser.
  const MAX_NEST = 4096, MAX_WORK = 3e7, MAX_ATTRS = 256;
  const words = s => new Set(s.split(' '));
  const H_VOID = words('area basefont bgsound br col embed frame hr image img input keygen link meta param source track wbr');
  // read as text up to their end tag, and dropped by rebuild with it
  const H_RAW = words('iframe noembed noframes script style textarea title xmp');
  // left out, their content kept (the parser ignores them in a body)
  const H_IGNORED = words('base body frameset head html');
  const H_FMT = words('a b big code em font i nobr s small strike strong tt u');
  const H_MARKER = words('applet caption marquee object td template th');
  const H_SPECIAL = words('address applet area article aside base basefont bgsound blockquote body br button caption ' +
    'center col colgroup dd details dir div dl dt embed fieldset figcaption figure footer form frame frameset h1 h2 h3 h4 ' +
    'h5 h6 head header hgroup hr html iframe img input keygen li link listing main marquee menu meta nav noembed noframes ' +
    'noscript object ol p param plaintext pre script search section select source style summary table tbody td template ' +
    'textarea tfoot th thead title tr track ul wbr xmp');
  const H_SCOPE = words('applet caption html marquee object table td template th');
  const M_TEXT = words('mi mn mo ms mtext');
  const S_IP = words('desc foreignobject title');
  const CLOSES_P = words('address article aside blockquote center details dialog dir div dl fieldset figcaption figure ' +
    'footer form h1 h2 h3 h4 h5 h6 header hgroup hr li listing main menu nav ol p plaintext pre search section summary ' +
    'table ul dd dt');
  // start tags before which the formatting elements that an end tag closed
  // are not reopened (they are before all others)
  const NO_REOPEN = new Set([...CLOSES_P, ...words('basefont bgsound caption col colgroup link meta param rb rp rt rtc ' +
                                                    'source tbody td template tfoot th thead tr track')]);
  const BREAKOUT = words('b big blockquote body br center code dd div dl dt em embed h1 h2 h3 h4 h5 h6 head hr i img li ' +
    'listing menu meta nobr ol p pre ruby s small span strike strong sub sup table tt u ul var');
  const IMPLIED_END = words('dd dt li optgroup option p rb rp rt rtc');
  const TABLE_PART = words('caption col colgroup tbody td tfoot th thead tr');
  const SCOPED_END = words('address applet article aside blockquote button center dd details dialog dir div dl dt ' +
    'fieldset figcaption figure footer form header hgroup listing main marquee menu nav object ol pre search section ' +
    'select summary template ul');
  const TABLE_END = words('caption colgroup table tbody td tfoot th thead tr');
  // the names that the tree builder treats as more than an ordinary element
  const H_ROLE = new Set([...H_VOID, ...H_RAW, ...H_IGNORED, ...H_FMT, ...H_MARKER, ...H_SPECIAL, ...H_SCOPE, ...CLOSES_P,
                          ...NO_REOPEN, ...BREAKOUT, ...IMPLIED_END, ...TABLE_PART, ...SCOPED_END, ...TABLE_END,
                          ...words('a button image keygen math nobr optgroup option rb rp rt rtc ruby select svg')]);
  // rebuild shows what is at most SHOWN elements inside another (it stops at
  // a depth of 64), and only the elements and attributes it knows
  const SHOWN = 64;
  const SVG_OK_LC = new Set([...SVG_OK].map(x => x.toLowerCase()));
  const SVG_ATTRS_LC = new Set([...SVG_ATTRS].map(x => x.toLowerCase()));
  const ARIA = /^aria-[a-z]+$/;
  const isAlpha = c => (c >= 65 && c <= 90) || (c >= 97 && c <= 122);
  const lower = x => /[A-Z]/.test(x) ? x.replace(/[A-Z]+/g, c => c.toLowerCase()) : x;
  const ATTR_NAME_OK = /^[^\t\n\f \/>"'<=\0]+$/;
  const NO_ATTRS = [];
  // (split and join: replace takes several times as long for a 4 MB text of <)
  const escapeAll = (t, c, by) => t.indexOf(c) < 0 ? t : t.split(c).join(by);

  function tidyHtml(src, limits) {
    const maxNest = (limits && limits.nest) || MAX_NEST, maxWork = (limits && limits.work) || MAX_WORK;
    const s = String(src).replace(/\r\n?/g, '\n');
    const n = s.length, maxOut = 6 * n + 65536;
    const out = [];
    let outLen = 0, work = 0;
    // the last thing written is text that ends as a character reference may
    // start: text written next, which the parser would join to it, comes
    // after an empty comment, as it came after a tag or a comment
    let refTail = false;
    const tooMuch = () => new Error('the HTML is too large for how deeply it nests');
    // the open elements, and for each kind of scope the indices of the
    // elements that bound it
    const stack = [];
    const at = new Map();                 // 'h:div' -> the indices of the open <div>
    const marks = { scope: [], button: [], list: [], table: [], special: [], nadp: [], marker: [], html: [], hns: [],
                    heading: [] };
    let P = [];                           // the formatting elements to reopen
    const last = a => a.length ? a[a.length - 1] : -1;
    const lastAt = k => { const a = at.get(k); return a ? last(a) : -1; };
    const top = () => stack[stack.length - 1];
    // the current node is foreign, and not an integration point for HTML
    const foreign = () => { const t = top(); return !!t && t.ns !== 'h' && !t.ip; };

    // the stack index of the element that rebuild would drop with its content
    // and that is left out with it, -1 if none is open
    let dropAt = -1;
    let formOpen = false;                 // the parser's form element pointer is set
    const hidden = () => dropAt >= 0 || stack.length > SHOWN;
    // what is hidden counts as if it were written: so does tidyHtml's own work
    function write(str, hide) {
      work += stack.length + 1;
      if (work > maxWork) throw tooMuch();
      if (hide === undefined ? hidden() : hide) return;
      out.push(str);
      outLen += str.length;
      refTail = false;
      if (outLen > maxOut) throw tooMuch();
    }
    // the attributes of an element that rebuild may keep, and none of an
    // element it does not make; dir as data-nb-dir, and a <bdi> is a <span
    // data-nb-bdi> (see rebuild: where the parser sees them, Chromium takes
    // time quadratic in their children)
    function attrText(attrs, name, ns) {
      let r = '';
      if (ns === 'm') return r;
      const h = ns === 'h', tag = name === 'image' ? 'img' : name;
      if (h && !HTML_OK.has(tag)) return r;
      const own = h && HTML_ATTRS[tag];
      const seen = new Set();
      if (h && tag === 'bdi') { r = ' data-nb-bdi=""'; seen.add('data-nb-bdi'); }
      for (let [a, v] of attrs) {
        if (h) {
          if (a === 'dir') a = 'data-nb-dir';
          else if (!(HTML_ATTRS['*'].includes(a) || own && own.includes(a) || ARIA.test(a) || a === 'data-nb-dir' ||
                     a === 'data-nb-bdi' && tag === 'span')) continue;
        } else if (!(SVG_ATTRS_LC.has(a) || ARIA.test(a) || a === 'xlink:href')) continue;
        if (seen.has(a) || !ATTR_NAME_OK.test(a)) continue;
        seen.add(a);
        if (seen.size > MAX_ATTRS) break;
        r += ' ' + a + '="' + escapeAll(escapeAll(v, '"', '&quot;'), '<', '&lt;') + '"';
      }
      return r;
    }
    // what an element of a name and namespace is: kinds.get('h:div')
    const kinds = new Map();
    function kindOf(name, ns, ip) {
      const key = ns + ':' + name + (ip ? ':ip' : '');
      let k = kinds.get(key);
      if (k) return k;
      const h = ns === 'h', f = [];
      ip = ip || (ns === 's' ? S_IP.has(name) : ns === 'm' && M_TEXT.has(name));
      const scope = h ? H_SCOPE.has(name) : ns === 'm' ? M_TEXT.has(name) || name === 'annotation-xml' : ip;
      if (scope) f.push(marks.scope, marks.button, marks.list);
      else if (h && name === 'button') f.push(marks.button);
      else if (h && (name === 'ol' || name === 'ul')) f.push(marks.list);
      if (h && (name === 'table' || name === 'template')) f.push(marks.table);
      if (h ? H_SPECIAL.has(name) : scope) {
        f.push(marks.special);
        if (!(h && (name === 'address' || name === 'div' || name === 'p'))) f.push(marks.nadp);
      }
      const marker = h && H_MARKER.has(name);
      if (marker) f.push(marks.marker);
      if (h || ip) f.push(marks.html);
      if (h) f.push(marks.hns);
      if (h && /^h[1-6]$/.test(name)) f.push(marks.heading);
      // the name written: an element that only rebuild unwraps, and that is no
      // more than an ordinary one to the parser, as one that does nothing
      const out = !h ? name : name === 'bdi' ? 'span' : HTML_OK.has(name) || H_ROLE.has(name) ? name : 'nb-w';
      // dropped by rebuild with its content
      const drop = ns === 'm' || (h ? HTML_DROP.has(name) : !SVG_OK_LC.has(name));
      kinds.set(key, k = { key: ns + ':' + name, fmt: h && H_FMT.has(name), marker, ip, flags: f, out, drop,
                           end: '</' + out + '>' });
      return k;
    }
    function entry(name, ns, text, attrs) {
      // an <annotation-xml> of HTML is an integration point
      const ip = ns === 'm' && name === 'annotation-xml' && !!attrs &&
        attrs.some(([a, v]) => a === 'encoding' && /^(text\/html|application\/xhtml\+xml)$/i.test(v));
      const k = kindOf(name, ns, ip);
      return { name, ns, text, kind: k, key: k.key, fmt: k.fmt, marker: k.marker, ip: k.ip, flags: k.flags,
               zombie: false, saved: null };
    }
    // an element that rebuild drops with its content: an HTML one in SVG too
    const drops = (kind, ns) => kind.drop || ns === 'h' && stack.length > 0 && top().ns === 's';
    // a void or self-closed element
    const leaf = (str, kind, ns) => write(str, hidden() || drops(kind, ns));
    function open(e) {
      const i = stack.length;
      if (i >= maxNest) throw new Error('the HTML nests more than ' + maxNest + ' elements deep');
      if (dropAt < 0 && drops(e.kind, e.ns)) dropAt = i;
      write('<' + e.kind.out + e.text + '>');
      stack.push(e);
      let a = at.get(e.key);
      if (!a) at.set(e.key, a = []);
      a.push(i);
      for (const m of e.flags) m.push(i);
      if (e.marker) { e.saved = P; P = []; }
    }
    function close() {
      const e = stack.pop();
      at.get(e.key).pop();
      for (const m of e.flags) m.pop();
      write(e.kind.end);
      if (dropAt === stack.length) dropAt = -1;
      if (e.marker) P = e.saved;
      return e;
    }
    // close the open elements down to index i, i included; the formatting
    // elements closed above it are reopened before the next text or inline
    // element, as the parser's list of them does, unless a marker closed
    function popTo(i) {
      let kept = [];
      while (stack.length > i) {
        const e = close();
        if (e.marker) kept = [];
        else if (stack.length > i && e.fmt && !e.zombie) kept.push(e);
      }
      if (kept.length) P = kept.reverse().concat(P);
      while (stack.length && top().zombie) close();
    }
    function reopen() {
      if (!P.length) return;
      const q = P;
      P = [];
      for (const e of q) open(entry(e.name, 'h', e.text, null));
    }
    function dropP(name) {
      work += P.length;
      for (let k = P.length - 1; k >= 0; k--) if (P[k].name === name) { P.splice(k, 1); return true; }
      return false;
    }
    const inScope = (i, m) => i >= 0 && i >= last(m);
    function closeP() { const i = lastAt('h:p'); if (inScope(i, marks.button)) popTo(i); }
    function anyOtherEnd(name) { const i = lastAt('h:' + name); if (inScope(i, marks.special)) popTo(i); }
    // the adoption agency, roughly: an end tag across a special element
    // (a <div> inside the <b>) leaves the element open until that one closes
    function endFormatting(name) {
      const i = lastAt('h:' + name);
      if (i > last(marks.marker) && !stack[i].zombie) {
        if (i < last(marks.scope)) return;
        if (last(marks.special) > i) { stack[i].zombie = true; return; }
        popTo(i);
        return;
      }
      if (!dropP(name)) anyOtherEnd(name);
    }
    function tablePart(name, attrs) {
      const t = last(marks.table);
      if (t < 0 || stack[t].name !== 'table') return;       // outside a table the parser ignores them
      const text = attrText(attrs, name, 'h');
      const sec = Math.max(lastAt('h:tbody'), lastAt('h:thead'), lastAt('h:tfoot')), row = lastAt('h:tr');
      if (name === 'col') {
        if (!(top().key === 'h:colgroup' && stack.length - 1 > t)) popTo(t + 1);
        leaf('<col' + text + '/>', kindOf('col', 'h'), 'h');
        return;
      }
      if (name === 'tr') popTo((sec > t ? sec : t) + 1);
      else if (name === 'td' || name === 'th') popTo((row > t ? row : sec > t ? sec : t) + 1);
      else popTo(t + 1);
      open(entry(name, 'h', text, attrs));
    }
    function startTag(name, attrs, selfClosing) {
      if (foreign()) {
        if (!(BREAKOUT.has(name) || name === 'font' && attrs.some(([a]) => a === 'color' || a === 'face' || a === 'size'))) {
          const t = top();
          const ns = t.ns === 'm' && t.name === 'annotation-xml' && name === 'svg' ? 's' : t.ns;
          const e = entry(name, ns, attrText(attrs, name, ns), attrs);
          if (selfClosing) leaf('<' + name + e.text + '></' + name + '>', e.kind, ns); else open(e);
          return;
        }
        popTo(last(marks.html) + 1);
      }
      if (H_IGNORED.has(name) || name === 'keygen') return;
      // a <form> inside one is ignored, until </form> (outside a <template>)
      const inTemplate = lastAt('h:template') >= 0;
      if (name === 'form' && formOpen && !inTemplate) return;
      if (TABLE_PART.has(name)) return tablePart(name, attrs);
      if (name === 'li' || name === 'dd' || name === 'dt') {
        const i = name === 'li' ? lastAt('h:li') : Math.max(lastAt('h:dd'), lastAt('h:dt'));
        if (inScope(i, marks.nadp)) popTo(i);
      }
      if (CLOSES_P.has(name)) closeP();
      if (/^h[1-6]$/.test(name) && stack.length && top().ns === 'h' && /^h[1-6]$/.test(top().name)) popTo(stack.length - 1);
      if (name === 'table') {
        // a table straight inside a table closes it
        const t = last(marks.table);
        if (t >= 0 && stack[t].name === 'table' && t > Math.max(lastAt('h:td'), lastAt('h:th'), lastAt('h:caption'))) popTo(t);
      }
      if (name === 'button') { const i = lastAt('h:button'); if (inScope(i, marks.scope)) popTo(i); }
      if (name === 'a') { if (lastAt('h:a') > last(marks.marker)) endFormatting('a'); dropP('a'); }
      // (the parser reopens the formatting elements before it looks for a <nobr>)
      if (name === 'nobr') { reopen(); if (inScope(lastAt('h:nobr'), marks.scope)) endFormatting('nobr'); }
      if ((name === 'option' || name === 'optgroup') && stack.length && top().key === 'h:option') popTo(stack.length - 1);
      if ((name === 'rb' || name === 'rtc' || name === 'rp' || name === 'rt') && inScope(lastAt('h:ruby'), marks.scope)) {
        while (stack.length && top().ns === 'h' && IMPLIED_END.has(top().name) &&
               !(top().name === 'rtc' && (name === 'rp' || name === 'rt'))) popTo(stack.length - 1);
      }
      if (!NO_REOPEN.has(name)) reopen();
      if (H_VOID.has(name)) { leaf('<' + name + attrText(attrs, name, 'h') + '/>', kindOf(name, 'h'), 'h'); return; }
      if (name === 'svg' || name === 'math') {
        const ns = name === 'svg' ? 's' : 'm', text = attrText(attrs, name, ns);
        const e = entry(name, ns, text, attrs);
        if (selfClosing) leaf('<' + name + text + '></' + name + '>', e.kind, ns); else open(e);
        return;
      }
      open(entry(name, 'h', attrText(attrs, name, 'h'), attrs));
      if (name === 'form' && !inTemplate) formOpen = true;
    }
    function endTag(name) {
      if (stack.length && top().ns !== 'h') {
        if (name === 'br' || name === 'p') popTo(last(marks.html) + 1);
        else {
          const i = Math.max(lastAt('s:' + name), lastAt('m:' + name));
          if (i > last(marks.hns)) { popTo(i); return; }
        }
      }
      if (H_IGNORED.has(name)) return;
      if (name === 'form' && lastAt('h:template') < 0) {
        const was = formOpen;
        formOpen = false;
        if (!was) return;
      }
      if (name === 'p') {
        const i = lastAt('h:p');
        if (inScope(i, marks.button)) popTo(i); else leaf('<p></p>', kindOf('p', 'h'), 'h');
        return;
      }
      if (name === 'br') { reopen(); leaf('<br/>', kindOf('br', 'h'), 'h'); return; }
      if (name === 'li') { const i = lastAt('h:li'); if (inScope(i, marks.list)) popTo(i); return; }
      if (/^h[1-6]$/.test(name)) { const i = last(marks.heading); if (inScope(i, marks.scope)) popTo(i); return; }
      if (SCOPED_END.has(name)) { const i = lastAt('h:' + name); if (inScope(i, marks.scope)) popTo(i); return; }
      if (TABLE_END.has(name)) { const i = lastAt('h:' + name); if (inScope(i, marks.table)) popTo(i); return; }
      if (H_FMT.has(name)) { endFormatting(name); return; }
      anyOtherEnd(name);
    }
    function text(t) {
      if (P.length && !foreign() && /[^\t\n\f ]/.test(t)) reopen();
      if (hidden()) { write('', true); return; }
      if (refTail) write('<!---->');
      write(escapeAll(t, '<', '&lt;'));
      refTail = /&[A-Za-z0-9#]*$/.test(t.slice(-48));
    }

    // the tokenizer
    // the ends of: spaces, a tag name, an attribute name, an unquoted value
    const SPACE = 1, NAME = 2, ATTR = 3, VALUE = 4;
    function scan(j, what) {
      for (; j < n; j++) {
        const c = s.charCodeAt(j);
        const space = c === 32 || c === 10 || c === 9 || c === 12;
        if (what === SPACE ? !space : space || c === 62 || what !== VALUE && (c === 47 || what === ATTR && c === 61)) break;
      }
      return j;
    }
    // the tag at i (after < or </), or null when the input ends inside it
    function readTag(i) {
      let j = scan(i, NAME);
      const name = lower(s.slice(i, j));
      let attrs = NO_ATTRS;
      for (;;) {
        j = scan(j, SPACE);
        if (j >= n) return null;
        const c = s[j];
        if (c === '>') return { name, attrs, selfClosing: false, next: j + 1 };
        if (c === '/') {
          if (s[j + 1] === '>') return { name, attrs, selfClosing: true, next: j + 2 };
          j++;
          continue;
        }
        const k = j;
        j = scan(c === '=' ? j + 1 : j, ATTR);
        const a = lower(s.slice(k, j));
        j = scan(j, SPACE);
        let v = '';
        if (s[j] === '=') {
          j = scan(j + 1, SPACE);
          const q = s[j];
          if (q === '"' || q === "'") {
            const e = s.indexOf(q, j + 1);
            if (e < 0) return null;
            v = s.slice(j + 1, e);
            j = e + 1;
          } else if (q !== '>') {
            const e = scan(j, VALUE);
            v = s.slice(j, e);
            j = e;
          }
        }
        if (attrs === NO_ATTRS) attrs = [];
        attrs.push([a, v]);
      }
    }
    const rawEnds = new Map();
    let bang = -2;                        // where the next --!> is, -1 if none
    // the text not written yet is s.slice(from, i)
    let i = 0, from = 0;
    const flush = end => { if (end > from) text(s.slice(from, end)); from = end; };
    const toGt = at => { const k = s.indexOf('>', at); return k < 0 ? n : k + 1; };
    const TAG = /<[A-Za-z\/!?]/g;           // what may not be text: < then a letter, /, ! or ?
    while (i < n) {
      TAG.lastIndex = i;
      const lt = TAG.test(s) ? TAG.lastIndex - 2 : -1;
      if (lt < 0) { i = n; break; }
      i = lt;
      const c = s.charCodeAt(i + 1);
      if (isAlpha(c)) {
        const t = readTag(i + 1);
        if (!t) break;                    // (a tag that the input ends inside is not one)
        flush(lt);
        from = i = t.next;
        if (!foreign() && (H_RAW.has(t.name) || t.name === 'plaintext')) {
          let re = t.name !== 'plaintext' && rawEnds.get(t.name);
          if (re === undefined) rawEnds.set(t.name, re = new RegExp('</' + t.name + '[\\t\\n\\f />]', 'gi'));
          let e = null;
          if (re) { re.lastIndex = i; const m = re.exec(s); e = m && readTag(m.index + 2); }
          from = i = e ? e.next : n;
          continue;
        }
        startTag(t.name, t.attrs, t.selfClosing);
      } else if (c === 47) {              // </
        const d = s.charCodeAt(i + 2);
        if (isAlpha(d)) {
          const t = readTag(i + 2);
          if (!t) break;
          flush(lt);
          from = i = t.next;
          endTag(t.name);
        } else if (d === 62) { flush(lt); from = i = lt + 3; }
        else if (i + 2 >= n) i = n;
        else { flush(lt); from = i = toGt(i + 2); }
      } else if (c === 33) {              // <!
        flush(lt);
        if (s.startsWith('<!--', i)) {
          if (s[i + 4] === '>') i += 5;
          else if (s[i + 4] === '-' && s[i + 5] === '>') i += 6;
          else {
            const a = s.indexOf('-->', i + 4);
            if (bang !== -1 && bang < i + 4) bang = s.indexOf('--!>', i + 4);
            i = a < 0 ? n : bang >= 0 && bang < a ? bang + 4 : a + 3;
          }
        } else if (stack.length && top().ns !== 'h' && s.startsWith('<![CDATA[', i)) {
          const e = s.indexOf(']]>', i + 9);
          text(s.slice(i + 9, e < 0 ? n : e).replace(/&/g, '&amp;'));
          i = e < 0 ? n : e + 3;
        } else i = toGt(i + 2);
        from = i;
      } else { flush(lt); from = i = toGt(i + 2); }   // <?
    }
    flush(i);
    return out.join('');
  }

  function sanitizeMarkup(str, kind, idPrefix, doc) {
    doc = doc || document;
    const prefix = idPrefix || '';
    const P = new (doc.defaultView && doc.defaultView.DOMParser || DOMParser)();
    // as many elements from uses as the markup has characters
    const budget = Math.max(2000, String(str).length);
    if (kind === 'svg') {
      const d = P.parseFromString(String(str), 'image/svg+xml');
      if (d.getElementsByTagName('parsererror').length || !d.documentElement || d.documentElement.namespaceURI !== SVG_NS)
        throw new Error('the SVG is not well-formed XML');
      const frag = doc.createDocumentFragment();
      const holder = doc.createElementNS(SVG_NS, 'g');
      const wrap = d.createElement('x');      // so that rebuild sees the root as a child
      wrap.appendChild(d.documentElement);
      rebuild(doc, wrap, holder, prefix, 0);
      limitUses(holder, budget);
      limitFilters(holder);
      while (holder.firstChild) frag.appendChild(holder.firstChild);
      return frag;
    }
    // (tidyHtml drops <base> too, which rebuild would drop, but parsing it
    // would set the parser document's base URI, which the page's CSP reports)
    const d = P.parseFromString('<!doctype html><html><head></head><body>' + tidyHtml(str), 'text/html');
    return rebuildHtml(d.body, prefix, budget, doc);
  }
  // what the page shows of a parsed body (the tests compare what it makes
  // of the markup itself and of what tidyHtml writes)
  function rebuildHtml(body, prefix, budget, doc) {
    doc = doc || document;
    const frag = doc.createDocumentFragment();
    rebuild(doc, body, frag, prefix || '', 0);
    limitUses(frag, budget || 2000);
    limitFilters(frag);
    return frag;
  }

  // ---- ids

  let idSeq = 0;
  function newId() {
    let r = '';
    try {
      const b = new Uint8Array(6);
      (globalThis.crypto || self.crypto).getRandomValues(b);
      for (const x of b) r += (x % 36).toString(36);
    } catch (e) { r = Math.random().toString(36).slice(2, 8); }
    return 'c' + r + (++idSeq).toString(36);
  }
  const ID_RE = /^[A-Za-z][A-Za-z0-9_-]{0,63}$/;

  // ---- JSON format

  const MIMES = new Set(['text/plain', 'text/html', 'image/svg+xml', 'text/markdown',
                         'image/png', 'image/jpeg', 'image/gif', 'image/webp']);
  // what Import accepts; a cell may be as large as the file (and keeps
  // at most OUTPUTS outputs, see limitOutputs)
  const LIMITS = { file: 5 * 1024 * 1024, cells: 5000, source: 5 * 1024 * 1024, outputs: 1000 };
  const NO_LIMITS = { file: Infinity, cells: Infinity, source: Infinity, outputs: Infinity };
  const str = (x, max) => typeof x === 'string' && x.length <= (max || Infinity);
  const strOrNull = x => x === null || x === undefined ? null : typeof x === 'string' ? x.slice(0, 4096) : null;

  // ERR as the kernel sends it (st/web-notebook.st): {text, kind, location,
  // form, line, column, chain}, each frame {where, proc, form}
  function cleanError(e) {
    if (!e || typeof e !== 'object' || typeof e.text !== 'string') return null;
    const chain = Array.isArray(e.chain) ? e.chain.slice(0, 200).filter(f => f && typeof f === 'object' && typeof f.where === 'string')
      .map(f => ({ where: f.where.slice(0, 200), proc: strOrNull(f.proc), form: strOrNull(f.form) })) : [];
    return {
      text: e.text.slice(0, 65536),
      kind: Array.isArray(e.kind) ? e.kind.filter(k => typeof k === 'string').slice(0, 16).map(k => k.slice(0, 64)) : [],
      location: strOrNull(e.location),
      form: Number.isInteger(e.form) && e.form > 0 ? e.form : e.form === 'print' ? 'print' : null,
      line: Number.isInteger(e.line) && e.line > 0 ? e.line : null,
      column: Number.isInteger(e.column) && e.column > 0 ? e.column : null,
      chain,
    };
  }
  function cleanOutput(o) {
    if (!o || typeof o !== 'object') return null;
    switch (o.k) {
    case 'stream':                      // the notebook has no stdin
      return (o.name === 'stdout' || o.name === 'stderr') && str(o.text)
        ? { k: 'stream', name: o.name, text: o.text } : null;
    case 'value':                       // text null: no value
      return o.text === null || str(o.text) ? { k: 'value', text: o.text } : null;
    case 'note':
      return str(o.text, 4096) ? Object.assign({ k: 'note', text: o.text }, o.bad === true ? { bad: true } : {}) : null;
    case 'display':
      return MIMES.has(o.mime) && str(o.data) ? { k: 'display', mime: o.mime, data: o.data, id: typeof o.id === 'string' ? o.id.slice(0, 256) : null } : null;
    case 'error': { const e = cleanError(o.error); return e ? { k: 'error', error: e } : null; }
    default: return null;
    }
  }
  function cleanDate(x) { return typeof x === 'string' && x.length < 40 && !isNaN(Date.parse(x)) ? x : null; }

  // a code cell's status is saved when it did not succeed
  const FAILED = new Set(['error', 'interrupted']);

  // nb: {title, created, modified, pharo (the kernel's version), cells}
  function toJson(nb) {
    return JSON.stringify({
      format: 'pharo-notebook',
      version: 1,
      meta: { title: nb.title || 'Untitled', created: nb.created || null, modified: nb.modified || null,
              pharo: nb.pharo || null },
      cells: nb.cells.map(c => {
        const o = { id: c.id, type: c.type, source: c.source };
        if (c.type === 'code' && Number.isInteger(c.count)) o.count = c.count;
        if (c.type === 'code' && FAILED.has(c.status)) o.status = c.status;
        if (c.outputs && c.outputs.length) o.outputs = c.outputs;
        return o;
      }),
    });
  }

  // At most MAX outputs: the first ones, a note saying how many were
  // left out, and the last one (which may be the error that ended the
  // cell).  OUTS itself when it has no more.
  function limitOutputs(outs, max) {
    if (!Array.isArray(outs) || outs.length <= max) return outs;
    if (max < 3) return outs.slice(0, max);
    const omitted = outs.length - (max - 1);
    return outs.slice(0, max - 2).concat([{ k: 'note', text: '… ' + omitted + ' outputs were left out' }],
                                         outs.slice(-1));
  }

  // OPTS.limits false: no limits, for what the page saved itself
  function fromJson(text, opts) {
    const lim = opts && opts.limits === false ? NO_LIMITS : LIMITS;
    if (typeof text !== 'string') throw new Error('not a notebook');
    if (text.length > lim.file) throw new Error('the file is larger than 5 MB');
    let j;
    try { j = JSON.parse(text); } catch (e) { throw new Error('not valid JSON: ' + e.message); }
    if (!j || typeof j !== 'object' || j.format !== 'pharo-notebook' || !Array.isArray(j.cells))
      throw new Error('not a Pharo notebook (format "pharo-notebook" expected)');
    const warnings = [];
    if (typeof j.version === 'number' && j.version > 1)
      warnings.push('This notebook was saved by a newer version (format ' + j.version + '); it was imported as far as possible.');
    if (j.cells.length > lim.cells) throw new Error('more than ' + lim.cells + ' cells');
    const meta = j.meta && typeof j.meta === 'object' ? j.meta : {};
    const seen = new Set();
    const cells = [];
    let dropped = 0, cut = 0;
    for (const c of j.cells) {
      if (!c || typeof c !== 'object' || (c.type !== 'code' && c.type !== 'markdown') || !str(c.source, lim.source)) { dropped++; continue; }
      let id = typeof c.id === 'string' && ID_RE.test(c.id) && !seen.has(c.id) ? c.id : newId();
      while (seen.has(id)) id = newId();
      seen.add(id);
      const cell = { id, type: c.type, source: c.source };
      if (c.type === 'code' && Number.isInteger(c.count) && c.count > 0 && c.count < 1e9) cell.count = c.count;
      if (c.type === 'code' && FAILED.has(c.status)) cell.status = c.status;
      if (Array.isArray(c.outputs)) {
        const all = c.outputs.map(cleanOutput).filter(Boolean);
        const outs = limitOutputs(all, lim.outputs);
        if (outs !== all) cut++;
        if (outs.length) cell.outputs = outs;
      }
      cells.push(cell);
    }
    if (dropped) warnings.push(dropped + ' invalid cell' + (dropped > 1 ? 's were' : ' was') + ' skipped.');
    if (cut) warnings.push(cut + (cut > 1 ? ' cells had' : ' cell had') + ' more than ' + lim.outputs +
                           ' outputs, so some were left out.');
    return {
      title: typeof meta.title === 'string' ? meta.title.slice(0, 200) : '',
      created: cleanDate(meta.created), modified: cleanDate(meta.modified),
      pharo: typeof meta.pharo === 'string' ? meta.pharo.slice(0, 64) : null,
      cells, warnings,
    };
  }

  return { renderMarkdown, sanitizeMarkup, tidyHtml, rebuildHtml, toJson, fromJson, cleanOutput, cleanError, limitOutputs,
           highlightLines, highlightDom, colorCode, newId, safeHref, safeImageSrc, slug, LIMITS, MAX_NEST, MAX_WORK,
           HIGHLIGHT_MAX };
});
