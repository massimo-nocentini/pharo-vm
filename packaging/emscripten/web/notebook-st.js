// notebook-st.js - the Smalltalk of the Notebook tab of the Console page (notebook.js)
//
// UMD: a CommonJS module in node (tests/wasm/notebook-lib.test.mjs), the
// global NotebookSt in the page.  No dependencies and no DOM: what is
// language-neutral is in notebook-lib.js (NotebookLib).
//
//   NotebookSt.scan(text, f)       -> {depth, stray, mismatch, open, ...}
//   NotebookSt.balance(text)       -> {depth, stray, mismatch, open, ok}
//   NotebookSt.balanceText(text)   -> '' or the hint of a cell, '1 unclosed ['
//   NotebookSt.highlight(text)     -> [[kind, text]], the colors of a cell
//   NotebookSt.toSt(nb)            -> the .st text of {title, cells}
//   NotebookSt.fromSt(text)        -> {title, cells, warnings}
//   NotebookSt.straightenQuotes(s) -> s with the quotes of a touch keyboard straight
//   NotebookSt.example()           -> {title, cells}, the example notebook
//   NotebookSt.FENCE               the languages of the Markdown code fences
//                                  that are Smalltalk, no language included
//   NotebookSt.LANGUAGE            'Smalltalk', as the labels of the cells say
//
// scan reads as Pharo's scanner does (RBScanner in Pharo 12, OCScanner in
// Pharo 15): strings '...' and comments "..." (a doubled quote is part of
// them), a character $x takes the next code point whatever it is, and the
// symbols #foo:bar:, #+ and #'...' are skipped.  ( [ { open and ) ] }
// close in code; in a literal array #( ... ) only ( and ) count, [ ] { }
// being symbols there, and in a byte array #[ ... ] only ] closes.  It runs
// in linear time, on any text.
//
// highlight reads the same way, and also the numbers, names and keywords as
// the scanner does, so that every token of the scanner has one color (the
// kinds are below highlight).  It runs in linear time too, on any text.
//
// The .st format is the chunk format that a fileIn reads ('x.st'
// asFileReference fileIn, or pharo Pharo.image st x.st): a chunk ends at a
// ! and a doubled !! is a ! of the chunk.  Each cell is a chunk, marked by a
// comment at its start, a code cell by "%%" and a Markdown cell by
// "%% [markdown] ...", the whole cell in the comment, its " doubled:
//
//   "Pharo notebook: A tour"!
//
//   "%%"
//   3 + 4!
//
//   "%% [markdown]
//   Say ""hi""!!"!
//
// A marker counts at the start of a chunk only, so a cell needs no escape
// but the doubled ! (and " in Markdown).  Filed in, the code cells run in
// order, as DoIts, and the rest are comments.  Each DoIt is compiled on its
// own, so the file runs as a script only when its cells declare the
// variables they use (| x |), or use globals: a variable that a cell assigns
// is the notebook's, not the file's.
//
// fromSt reads the chunks as Pharo's ChunkFileFormatParser does: a chunk
// that begins with a ! is a preamble, which is not evaluated, and that of a
// !Foo methodsFor: 'x'! section is followed by its methods, up to an empty
// chunk.  The DoIts are the cells, a code cell each when they have no
// marker; method definitions and class comments are left out, with a
// warning.

(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.NotebookSt = factory();
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  const LETTER = /[\p{L}_]/u, WORD = /[\p{L}\p{N}_]/u;
  // the characters of binary selectors, as the scanner's classification
  // table has them (! too: a cell is no chunk)
  const BINARY = /[-+*/\\~<>=@%|&?,!\u00b1\u00b7\u00d7\u00f7]/;
  const LANGUAGE = 'Smalltalk';
  const FENCE = /^(|smalltalk|st|pharo)$/i;

  // ---- scanning

  // Calls f(kind, i[, depth]) for the brackets and line breaks outside
  // strings, comments and characters: 'open' at an opener (the ( of #( and
  // the [ of #[), 'close' at a closer, 'newline' with the depth there, and
  // 'comment' at the closing " of a comment (at the end of the text when it
  // has none).  Answers the state at the end: depth, the brackets still
  // open; stray, the closers with nothing open; mismatch, the closers of
  // another opener, and in a byte array the brackets other than ]; open,
  // whether a string, a comment or a character is unfinished.  And for
  // balanceText: unclosed, the character that began it (' " or $), pending,
  // the closers that the open brackets need, innermost last, and strays, the
  // stray closers.
  function scan(text, f) {
    const t = String(text), n = t.length;
    const stack = [];                  // ( [ { in code, L a literal array, B a byte array
    let stray = 0, mismatch = 0, unclosed = null, strays = '';
    for (let i = 0; i < n; i++) {
      const c = t[i], top = stack[stack.length - 1];
      if (c === '"' || c === '\'') {   // a comment or a string; a doubled quote is in it
        let j = i + 1;
        for (;;) {
          j = t.indexOf(c, j);
          if (j < 0) { unclosed = c; j = n; break; }
          if (t[j + 1] === c) { j += 2; continue; }
          break;
        }
        if (f && c === '"') f('comment', j);
        i = j;
        continue;
      }
      if (c === '$') {                 // a character: the next code point, whatever it is
        if (i + 1 >= n) { unclosed = c; break; }
        const x = t.charCodeAt(i + 1);
        i += x >= 0xd800 && x < 0xdc00 && i + 2 < n ? 2 : 1;
        continue;
      }
      if (c === '#') {
        const d = t[i + 1];
        if (d === '(' || d === '[') {
          stack.push(d === '(' ? 'L' : 'B');
          if (f) f('open', i + 1);
          i++;
          continue;
        }
        // #'...' and ##foo: the string or the symbol comes next.  The
        // characters of #foo:bar: and #+ are none of the brackets
        let j = i + 1;
        if (d && LETTER.test(d)) { while (j < n && (WORD.test(t[j]) || t[j] === ':')) j++; i = j - 1; }
        else if (d && BINARY.test(d)) { while (j < n && BINARY.test(t[j])) j++; i = j - 1; }
        continue;
      }
      if (top === 'B') {
        if (c === ']') { stack.pop(); if (f) f('close', i); }
        else if ('()[{}'.includes(c)) mismatch++;
        else if (c === '\n' && f) f('newline', i, stack.length);
        continue;
      }
      if (top === 'L') {               // [ ] { } are symbols here
        if (c === '(') { stack.push('L'); if (f) f('open', i); }
        else if (c === ')') { stack.pop(); if (f) f('close', i); }
        else if (c === '\n' && f) f('newline', i, stack.length);
        continue;
      }
      if (c === '(' || c === '[' || c === '{') { stack.push(c); if (f) f('open', i); }
      else if (c === ')' || c === ']' || c === '}') {
        if (!stack.length) { stray++; if (!strays.includes(c)) strays += c; }
        else if ('([{'.indexOf(stack.pop()) !== ')]}'.indexOf(c)) mismatch++;
        if (f) f('close', i);
      } else if (c === '\n' && f) f('newline', i, stack.length);
    }
    const pending = stack.map(o => (o === '(' || o === 'L' ? ')' : o === '{' ? '}' : ']')).join('');
    return { depth: stack.length, stray, mismatch, open: unclosed !== null, unclosed, pending, strays };
  }

  function balance(text) {
    const s = scan(String(text || ''));
    return { depth: s.depth, stray: s.stray, mismatch: s.mismatch, open: s.open,
             ok: !s.depth && !s.stray && !s.mismatch && !s.open };
  }

  const OPENER = { ')': '(', ']': '[', '}': '{' };
  const plural = (k, one, many) => k + ' ' + (k > 1 ? many : one);

  // What the hint under a code cell says, '' when the cell is balanced
  function balanceText(text) {
    const s = scan(String(text || ''));
    if (s.unclosed === '"') return 'unclosed comment';
    if (s.unclosed === '\'') return 'unclosed string';
    if (s.unclosed === '$') return 'a $ with no character after it';
    if (s.stray) return s.strays.length === 1 ? s.stray + ' extra ' + s.strays : plural(s.stray, 'extra closer', 'extra closers');
    if (s.mismatch) return plural(s.mismatch, 'mismatched bracket', 'mismatched brackets');
    if (s.depth) {
      const one = s.pending.split('').every(c => c === s.pending[0]);
      return one ? s.depth + ' unclosed ' + OPENER[s.pending[0]] : s.depth + ' unclosed brackets';
    }
    return '';
  }

  // ---- highlighting

  const SPACE = /\s/;
  // LETTER, WORD, SPACE and an upper-case letter, ASCII first
  const ascii = c => !(c > '\u007f');
  const isLetter = c => (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c === '_' || (!ascii(c) && LETTER.test(c));
  const isWord = c => isLetter(c) || (c >= '0' && c <= '9') || (!ascii(c) && WORD.test(c));
  const capital = c => (c >= 'A' && c <= 'Z') || (!ascii(c) && /\p{Lu}/u.test(c));
  const separator = c => c === ' ' || c === '\n' || c === '\t' || c === '\r' || c === '\f' || (!ascii(c) && SPACE.test(c));
  const PSEUDO = new Set(['self', 'super', 'thisContext']);
  const CONSTANT = new Set(['nil', 'true', 'false']);
  // the < of a pragma: <primitive: 60>, <script>
  const PRAGMA = /<[\p{L}_][\p{L}\p{N}_]*(?::(?!=)|>)/uy;

  // The value of a digit (0-9, then a-z or A-Z), Infinity when it is none
  function digit(c) {
    const x = c === undefined ? -1 : c.charCodeAt(0) | 0x20;
    return c >= '0' && c <= '9' ? +c : x >= 0x61 && x <= 0x7a ? x - 0x61 + 10 : Infinity;
  }
  // The end of the digits of RADIX from j, a _ between two of them (as in
  // Pharo 15: Pharo 12 reads 1_000 as 1 and _000), or j when there is none
  function digits(t, j, radix) {
    if (digit(t[j]) >= radix) return j;
    for (j++; ; j++) {
      if (t[j] === '_' && digit(t[j + 1]) < radix) j++;
      else if (digit(t[j]) >= radix) return j;
    }
  }
  // The end of the number that begins with the digit at i, as the scanner
  // reads it (NumberParser): 16r1F, 16r-1F, 2r1e10, 1.5e-3, 1.5s2, 3s; 1e,
  // 1.e5 and 3sqrt are a number and a name
  function numberEnd(t, i) {
    let j = digits(t, i, 10), radix = 10;
    if (t[j] === 'r') {
      const r = parseInt(t.slice(i, j).replace(/_/g, ''), 10), k = t[j + 1] === '-' ? j + 2 : j + 1;
      if (r >= 2 && digits(t, k, r) > k) { radix = r; j = digits(t, k, r); }
    }
    if (t[j] === '.' && digit(t[j + 1]) < radix) j = digits(t, j + 1, radix);
    const c = t[j];
    if (c === 'e' || c === 'd' || c === 'q') {
      const k = t[j + 1] === '-' ? j + 2 : j + 1, e = digits(t, k, 10);
      if (e > k) return e;
    }
    if (c === 's') {
      const e = digits(t, j + 1, 10);
      if (e > j + 1 || !LETTER.test(t[j + 1] || '')) return e > j + 1 ? e : j + 1;
    }
    return j;
  }
  const isDigit = c => c >= '0' && c <= '9';

  // [kind, text] pieces whose texts add up to the Smalltalk source, in
  // linear time; unterminated strings and comments run to the end.  Kinds:
  // '' (spaces, names, binary selectors, block parameters), comment,
  // string, char ($x), number (with its sign where a value starts, so that
  // 3-2 is not 3 and -2), quote (a symbol, a literal or byte array, its
  // brackets and its bare words), constant (nil true false), special (self
  // super thisContext ^ :=), keyword (at: and at:put:), paren (( ) [ ] { }
  // . ; | and the < > of a pragma), global (a capitalized name).  Strings,
  // comments, characters and brackets are found as scan() finds them.
  function highlight(text) {
    const t = String(text), n = t.length, kinds = [], ends = [];
    const stack = [];                  // ( [ { in code, L a literal array, B a byte array
    let i = 0, operand = false, pragma = false;   // operand: a value just ended, so a - is binary
    const put = (k, j) => {            // the piece of kind k up to j, or more of the last one
      if (kinds.length && kinds[kinds.length - 1] === k) ends[ends.length - 1] = j;
      else { kinds.push(k); ends.push(j); }
      i = j;
    };
    // the end of the string or comment that opens at from
    const quoted = (q, from) => {
      for (let j = from + 1; ;) {
        j = t.indexOf(q, j);
        if (j < 0) return n;
        if (t[j + 1] === q) { j += 2; continue; }
        return j + 1;
      }
    };
    // the end of the name at j, or of the keywords there (at:put:), with
    // their colons
    const keywords = j => {
      while (j < n && isWord(t[j])) j++;
      if (t[j] !== ':' || t[j + 1] === '=') return j;
      for (j++; isLetter(t[j] || '');) {
        let k = j + 1;
        while (k < n && isWord(t[k])) k++;
        if (t[k] !== ':' || t[k + 1] === '=') break;
        j = k + 1;
      }
      return j;
    };
    while (i < n) {
      const c = t[i], d = t[i + 1], top = stack[stack.length - 1];
      if (separator(c)) { let j = i + 1; while (j < n && separator(t[j])) j++; put('', j); continue; }
      if (c === '"') { put('comment', quoted('"', i)); continue; }
      const was = operand;
      operand = true;                  // unless what follows says otherwise
      if (c === '\'') { put('string', quoted('\'', i)); continue; }
      if (c === '$') {                 // the next code point, whatever it is
        const x = t.charCodeAt(i + 1);
        put('char', Math.min(n, x >= 0xd800 && x < 0xdc00 ? i + 3 : i + 2));
        continue;
      }
      if (c === '#') {
        let j = i + 1;
        while (t[j] === '#') j++;      // ##foo, ##( as #(
        if (t[j] === '(' || t[j] === '[') { stack.push(t[j] === '(' ? 'L' : 'B'); put('quote', j + 1); operand = false; continue; }
        if (t[j] === '\'') { put('quote', quoted('\'', j)); continue; }
        if (isLetter(t[j] || '')) { while (j < n && (isWord(t[j]) || t[j] === ':')) j++; put('quote', j); continue; }
        if (BINARY.test(t[j] || '')) { while (j < n && BINARY.test(t[j])) j++; put('quote', j); continue; }
        put('', j); operand = false;   // a # alone, an error
        continue;
      }
      if (isDigit(c) || (c === '-' && isDigit(d) && (!was || top === 'L' || top === 'B'))) {
        put('number', numberEnd(t, c === '-' ? i + 1 : i));
        continue;
      }
      if (top === 'B') {               // only numbers and the ]
        if (c === ']') { stack.pop(); put('quote', i + 1); } else { put('', i + 1); operand = false; }
        continue;
      }
      if (top === 'L') {               // all is data: [ ] { } ^ ; . := are symbols
        if (c === '(') { stack.push('L'); put('quote', i + 1); operand = false; continue; }
        if (c === ')') { stack.pop(); put('quote', i + 1); continue; }
        let j = i + 1;
        if (isLetter(c)) {
          j = keywords(i);
          put(t[j - 1] !== ':' && CONSTANT.has(t.slice(i, j)) ? 'constant' : 'quote', j);
          continue;
        }
        if (BINARY.test(c)) while (j < n && BINARY.test(t[j])) j++;
        else if (c === ':' && d === '=') j++;
        put('quote', j); operand = false;
        continue;
      }
      if (c === '(' || c === '[' || c === '{') { stack.push(c); put('paren', i + 1); operand = false; continue; }
      if (c === ')' || c === ']' || c === '}') { stack.pop(); put('paren', i + 1); continue; }
      if (isLetter(c)) {
        let j = i + 1;
        while (j < n && isWord(t[j])) j++;
        if (t[j] === ':' && t[j + 1] !== '=') { put('keyword', keywords(i)); operand = false; continue; }
        const w = t.slice(i, j);
        put(PSEUDO.has(w) ? 'special' : CONSTANT.has(w) ? 'constant' : capital(w[0]) ? 'global' : '', j);
        continue;
      }
      operand = false;
      if (c === ':' && d === '=') { put('special', i + 2); continue; }
      if (c === '^') { put('special', i + 1); continue; }
      if (c === '.' || c === ';' || c === '|') { put('paren', i + 1); continue; }
      if (!stack.length && (pragma ? c === '>' : c === '<' && (PRAGMA.lastIndex = i, PRAGMA.test(t)))) {
        pragma = !pragma;
        put('paren', i + 1);
        continue;
      }
      if (c === ':' && isLetter(d || '')) {      // a block parameter
        let j = i + 2;
        while (j < n && isWord(t[j])) j++;
        put('', j); operand = true;
        continue;
      }
      if (BINARY.test(c)) {
        let j = i + 1;
        while (j < n && BINARY.test(t[j]) && t[j] !== '|') j++;
        put('', j);
        continue;
      }
      put('', i + 1);                  // a lone : and anything else
    }
    return kinds.map((k, x) => [k, t.slice(x ? ends[x - 1] : 0, ends[x])]);
  }

  // ---- quotes

  // Touch keyboards make curly quotes of ' and ".  Outside strings and
  // comments they are never what the user meant, so they become ' and "
  // again.  Inside a string or a comment they stay, but a string or a
  // comment that a curly quote opened is closed by one too.  The character
  // after a $ is straightened as well: $' typed on such a keyboard.
  const SINGLE = c => c === '\u2018' || c === '\u2019';
  const DOUBLE = c => c === '\u201c' || c === '\u201d';
  function straightenQuotes(s) {
    const t = String(s), n = t.length;
    let out = '', inside = null, curly = false;  // inside: the quote of the string or comment
    for (let i = 0; i < n; i++) {
      const c = t[i];
      if (inside) {
        if (c === inside || (curly && (inside === '\'' ? c === '\u2019' : c === '\u201d'))) { out += inside; inside = null; }
        else out += c;
        continue;
      }
      if (c === '$' && i + 1 < n) {
        const d = t[++i];
        out += c + (SINGLE(d) ? '\'' : DOUBLE(d) ? '"' : d);
      } else if (c === '\'' || SINGLE(c)) { out += '\''; inside = '\''; curly = c !== '\''; }
      else if (c === '"' || DOUBLE(c)) { out += '"'; inside = '"'; curly = c !== '"'; }
      else out += c;
    }
    return out;
  }

  // ---- the .st format

  const isSpace = c => c === ' ' || c === '\t' || c === '\n' || c === '\f';
  const bang = s => s.replace(/!/g, '!!');
  const quote2 = s => s.replace(/"/g, '""');
  const lines = s => String(s == null ? '' : s).replace(/\r\n?/g, '\n');

  // s without its leading blank lines and its trailing white space
  function trimBlankLines(s) {
    let a = 0, b = s.length;
    while (b > 0 && isSpace(s[b - 1])) b--;
    for (let k = 0; k < b && isSpace(s[k]); k++) if (s[k] === '\n') a = k + 1;
    return s.slice(a, b);
  }

  function toSt(nb) {
    const title = lines(nb.title || 'Untitled').replace(/\s*\n\s*/g, ' ');
    const out = ['"Pharo notebook: ' + bang(quote2(title)) + '"!', ''];
    for (const c of nb.cells) {
      if (c.type === 'markdown') out.push('"%% [markdown]\n' + bang(quote2(lines(c.source))) + '"!');
      else out.push('"%%"\n' + bang(lines(c.source)) + '!');
      out.push('');
    }
    return out.join('\n');
  }

  // The end of the comment that begins at s[0] (the index after its closing
  // "), or -1 when it does not end
  function commentEnd(s) {
    for (let j = 1; ;) {
      j = s.indexOf('"', j);
      if (j < 0) return -1;
      if (s[j + 1] === '"') { j += 2; continue; }
      return j + 1;
    }
  }
  const HEAD = '"Pharo notebook:';
  const MARK_MD = /^%%[ \t]*\[[ \t]*markdown[ \t]*\][ \t]*\n?/i;

  // The cells of a chunk: a DoIt, possibly with a marker
  function cellOf(body) {
    const end = body[0] === '"' ? commentEnd(body) : -1;
    if (end > 0 && body.startsWith('"%%')) {
      const text = body.slice(1, end - 1), rest = body.slice(end);
      const md = MARK_MD.exec(text);
      if (md && !rest.trim()) return { type: 'markdown', source: trimBlankLines(text.slice(md[0].length).replace(/""/g, '"')) };
      if (!md) return { type: 'code', source: trimBlankLines(rest.replace(/^[ \t]*\n?/, '')) };
    }
    return { type: 'code', source: trimBlankLines(body) };
  }

  function fromSt(text) {
    const t = lines(text).replace(/^\uFEFF/, ''), n = t.length;
    let i = 0;
    const skip = () => { while (i < n && isSpace(t[i])) i++; };
    // the next chunk, as Pharo's ChunkReadStream>>next reads it: from the
    // first character that is not a separator up to the next single !
    const chunk = () => {
      skip();
      let s = '', from = i;
      for (;;) {
        const j = t.indexOf('!', i);
        if (j < 0) { i = n; return s + t.slice(from); }
        if (t[j + 1] === '!') { s += t.slice(from, j + 1); i = from = j + 2; continue; }
        i = j + 1;
        return s + t.slice(from, j);
      }
    };
    let title = '', methods = 0, comments = 0;
    const cells = [];
    for (;;) {
      skip();
      if (i >= n) break;
      if (t[i] === '!') {
        // a preamble, as Pharo's ChunkFileFormatParser reads it: methods
        // up to an empty chunk, or one class comment, or one organization
        i++;
        const pre = chunk();
        if (/\bmethodsFor:/.test(pre)) { while (i < n && chunk() !== '') methods++; }
        else if (/\bcommentStamp:/.test(pre)) { chunk(); comments++; }
        else if (/\breorganize\b/.test(pre)) chunk();
        continue;
      }
      const body = chunk();
      if (!cells.length && !title && body.startsWith(HEAD)) {
        const end = commentEnd(body);
        if (end > 0 && !body.slice(end).trim()) {
          title = body.slice(HEAD.length, end - 1).replace(/""/g, '"').trim().slice(0, 200);
          continue;
        }
      }
      if (body.trim()) cells.push(cellOf(body));
    }
    const warnings = [];
    if (methods || comments) {
      const what = [methods && plural(methods, 'method definition', 'method definitions'),
                    comments && plural(comments, 'class comment', 'class comments')].filter(Boolean).join(' and ');
      // such as '3 method definitions and 1 class comment were left out: ...'
      warnings.push(what + (methods + comments > 1 ? ' were' : ' was') + ' left out: a notebook keeps only the DoIts ' +
                    'of a .st file. To load them, upload the file and file it in from a cell.');
    }
    return { title, cells, warnings };
  }

  // ---- the example notebook

  function example() {
    const md = s => ({ type: 'markdown', source: s });
    const code = s => ({ type: 'code', source: s });
    return { title: 'A tour of the notebook', cells: [
      md('# Pharo notebook\n\n' +
         'Write Smalltalk in the code cells and run them with **Shift+Enter** (run and move on) or **Ctrl+Enter** ' +
         '(run in place); **Run all** runs the whole notebook from the top. The statements of a cell run one after ' +
         'the other, and the value of the last one is shown. A variable that a cell assigns is kept for the next ' +
         'cells, as in a Playground.\n\n' +
         '- The notebook has a Pharo of its own, started from the image of the **Console** tab as it was last saved: ' +
         'what you do here does not change that image. **Restart** starts the notebook\'s Pharo afresh.\n' +
         '- Press **Esc** for command mode, then **?** for every shortcut.\n' +
         '- The notebook is saved in this browser as you type. **More → Export** keeps a copy as `.st` or as `.json` ' +
         '(with outputs).\n' +
         '- **More → Upload files** (or dropping files here) copies them into `/pharo`; then ' +
         '`\'file.st\' asFileReference fileIn`.'),
      code('3 + 4'),
      code('100 factorial printString size'),
      code('(1 to: 10) collect: [ :i | i * i ]'),
      code('"A class, defined and used in the same cell"\n' +
           'Object << #Counter\n' +
           '\tslots: { #count };\n' +
           '\tpackage: \'Notebook-Example\';\n' +
           '\tinstall.\n' +
           'Counter compile: \'increment\n' +
           '\tcount := self count + 1\'.\n' +
           'Counter compile: \'count\n' +
           '\t^ count ifNil: [ 0 ]\'.\n' +
           '\n' +
           'counter := Counter new.\n' +
           '3 timesRepeat: [ counter increment ].\n' +
           'counter count'),
      md('## Rich output\n\n' +
         'The class `Notebook` shows values as tables, HTML, SVG, Markdown or pictures: `Notebook table:header:`, ' +
         '`html:`, `svg:`, `markdown:`, `image:` and `show:id:`. A Form or a Morph that a cell answers is shown as ' +
         'what it looks like, and what the cell writes on the `Transcript` comes out under it. Markup is cleaned ' +
         'of scripts and external resources before it is shown.'),
      code('Notebook\n' +
           '\ttable: ((1 to: 6) collect: [ :n | { n. n * n. n factorial } ])\n' +
           '\theader: #(\'n\' \'n squared\' \'n factorial\')'),
      code('"An SVG bar chart; currentColor follows the page theme"\n' +
           '| values height |\n' +
           'values := #(3 7 4 9 5 8 2 6).\n' +
           'height := 120.\n' +
           'Notebook svg: (String streamContents: [ :svg |\n' +
           '\tsvg << \'<svg width="\' << (36 * values size) printString\n' +
           '\t\t<< \'" height="\' << (height + 20) printString << \'">\'.\n' +
           '\tvalues withIndexDo: [ :v :i | | x h |\n' +
           '\t\tx := i - 1 * 36.\n' +
           '\t\th := v * height // values max.\n' +
           '\t\tsvg << \'<rect x="\' << x printString << \'" y="\' << (height - h) printString\n' +
           '\t\t\t<< \'" width="28" height="\' << h printString << \'" rx="3" fill="currentColor" opacity="0.6"/>\'\n' +
           '\t\t\t<< \'<text x="\' << (x + 14) printString << \'" y="\' << (height + 15) printString\n' +
           '\t\t\t<< \'" font-size="11" text-anchor="middle" fill="currentColor">\' << v printString << \'</text>\' ].\n' +
           '\tsvg << \'</svg>\' ])'),
      code('Transcript show: \'Hello from the Transcript\'; cr'),
      code('(Form extent: 160 @ 48 depth: 32)\n' +
           '\tfillColor: Color orange;\n' +
           '\tyourself'),
      code('"show:id: replaces an output in place"\n' +
           '0 to: 5 do: [ :i |\n' +
           '\tNotebook\n' +
           '\t\tshow: (Notebook html: \'<progress max="5" value="\', i printString, \'"></progress> \', i printString, \' of 5\')\n' +
           '\t\tid: \'progress\'.\n' +
           '\t(Delay forMilliseconds: 400) wait ].\n' +
           '\'done\''),
      code('"An error stops Run all; its call history says where it happened"\n' +
           '#(1 2 3) at: 5'),
      code('"Run all stopped at the error above, so this cell was not run: run it by itself with Ctrl+Enter"\n' +
           'Notebook variables'),
    ] };
  }

  return { scan, balance, balanceText, highlight, toSt, fromSt, straightenQuotes, example, FENCE, LANGUAGE };
});
