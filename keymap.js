// keymap.js - DOM keyboard and mouse events as the input records of the Pharo world
//
// UMD: a CommonJS module in node (tests/wasm/keymap.test.mjs), the global
// PharoKeymap on the world page (world.js).  No DOM is needed: an event is
// any object with the fields of a KeyboardEvent (key, code, shiftKey,
// ctrlKey, altKey, metaKey, repeat, isComposing, getModifierState).
//
//   const os = PharoKeymap.platform();    // 'mac', 'windows' or 'other'
//   const k = PharoKeymap.key(event, os);
//   // {sym, scan, mods, text, shortcut, prevent, paste}
//
// os may also be a boolean, as isMac() answers: true is 'mac', false 'other'.
//
// The image (packaging/emscripten/st/OSWindow-Web) reads the keys as an SDL2
// window would get them:
//
//   sym   the SDL keycode: the character of the key without Shift, in the
//         layout of the user (a lower case letter, '1' for Shift+1), or for a
//         key that types no character its SDL scancode | 0x40000000
//         (ArrowLeft 0x40000050); Enter 13, Tab 9, Backspace 8, Escape 27,
//         Delete 127 and Space 32 are characters
//   scan  the SDL scancode, the USB HID usage of the physical key (code)
//   mods  1 shift, 2 ctrl, 4 alt, 8 cmd.  On macOS Cmd is the shortcut key,
//         which the image (a Unix one for Pharo) expects as Ctrl: Cmd is
//         sent as Ctrl.  Elsewhere Meta (the Windows key) is cmd.  AltGr
//         types characters, and is neither Ctrl nor Alt; on Windows, Ctrl+Alt
//         is AltGr when it types a character the key has neither alone nor
//         with Shift (see altGraph).
//   text  the code point the key types, or 0.  As with SDL the text is an
//         event of its own (textInput), sent for every key that produces a
//         printable character, with Shift, Option or AltGr too, but not for
//         a shortcut (Ctrl or Cmd held) nor for Escape, Enter, Tab or
//         Backspace, which the image gets as keys.
//
// prevent says whether the page keeps the browser from acting on the key
// (the world handles it): every key but the modifiers and the keys of a
// composition, and Ctrl/Cmd chords but copy, cut and paste, whose events
// the page needs.  paste says that the key is Ctrl/Cmd+V: the page sends
// it after the clipboard text of the paste event.
//
// buttons(domButtons) and button(domButton) map the mouse buttons as SDL
// numbers them: buttons 1 left, 2 middle, 3 right; held masks 1 left,
// 2 middle, 4 right (DOM: 1 left, 2 right, 4 middle).

(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.PharoKeymap = factory();
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  const MASK = 0x40000000;              // SDLK_SCANCODE_MASK
  const SHIFT = 1, CTRL = 2, ALT = 4, CMD = 8;

  // SDL scancodes (USB HID usages) of KeyboardEvent.code
  const SCAN = {
    Enter: 40, Escape: 41, Backspace: 42, Tab: 43, Space: 44,
    Minus: 45, Equal: 46, BracketLeft: 47, BracketRight: 48, Backslash: 49,
    IntlHash: 50, Semicolon: 51, Quote: 52, Backquote: 53, Comma: 54,
    Period: 55, Slash: 56, CapsLock: 57,
    PrintScreen: 70, ScrollLock: 71, Pause: 72, Insert: 73, Home: 74,
    PageUp: 75, Delete: 76, End: 77, PageDown: 78,
    ArrowRight: 79, ArrowLeft: 80, ArrowDown: 81, ArrowUp: 82,
    NumLock: 83, NumpadDivide: 84, NumpadMultiply: 85, NumpadSubtract: 86,
    NumpadAdd: 87, NumpadEnter: 88, Numpad0: 98, NumpadDecimal: 99,
    IntlBackslash: 100, ContextMenu: 101, Power: 102, NumpadEqual: 103,
    Help: 117, NumpadComma: 133, IntlRo: 135, KanaMode: 136, IntlYen: 137,
    Convert: 138, NonConvert: 139, Lang1: 144, Lang2: 145,
    ControlLeft: 224, ShiftLeft: 225, AltLeft: 226, MetaLeft: 227,
    ControlRight: 228, ShiftRight: 229, AltRight: 230, MetaRight: 231,
    OSLeft: 227, OSRight: 231,
  };
  for (let i = 0; i < 26; i++) SCAN['Key' + String.fromCharCode(65 + i)] = 4 + i;
  for (let i = 1; i <= 9; i++) {
    SCAN['Digit' + i] = 29 + i;
    SCAN['Numpad' + i] = 88 + i;
  }
  SCAN.Digit0 = 39;
  for (let i = 1; i <= 12; i++) SCAN['F' + i] = 57 + i;
  for (let i = 13; i <= 24; i++) SCAN['F' + i] = 91 + i;

  // The characters of the keys of a US keyboard, without Shift: the keycode
  // of a key whose character the layout or a modifier changed
  const US = {
    Space: ' ', Minus: '-', Equal: '=', BracketLeft: '[', BracketRight: ']',
    Backslash: '\\', Semicolon: ';', Quote: "'", Backquote: '`', Comma: ',',
    Period: '.', Slash: '/',
  };
  for (let i = 0; i < 26; i++) US['Key' + String.fromCharCode(65 + i)] = String.fromCharCode(97 + i);
  for (let i = 0; i <= 9; i++) US['Digit' + i] = String(i);
  // and with Shift, but the letters
  const US_SHIFTED = {
    Digit1: '!', Digit2: '@', Digit3: '#', Digit4: '$', Digit5: '%', Digit6: '^', Digit7: '&',
    Digit8: '*', Digit9: '(', Digit0: ')', Minus: '_', Equal: '+', BracketLeft: '{', BracketRight: '}',
    Backslash: '|', Semicolon: ':', Quote: '"', Backquote: '~', Comma: '<', Period: '>', Slash: '?',
  };

  // The keys whose SDL keycode is a character, by KeyboardEvent.key
  const CHAR_KEYS = { Enter: 13, Tab: 9, Backspace: 8, Escape: 27, Delete: 127 };

  // The keys that type no character, by KeyboardEvent.key: their scancode,
  // when the code of the event does not say better (a modifier's side)
  const NAMED = {
    ArrowLeft: 80, ArrowRight: 79, ArrowUp: 82, ArrowDown: 81,
    Home: 74, End: 77, PageUp: 75, PageDown: 78, Insert: 73,
    CapsLock: 57, NumLock: 83, ScrollLock: 71, Pause: 72, PrintScreen: 70,
    ContextMenu: 101, Help: 117, Clear: 83,
    Shift: 225, Control: 224, Alt: 226, AltGraph: 230, Meta: 227, OS: 227,
  };
  for (let i = 1; i <= 24; i++) NAMED['F' + i] = SCAN['F' + i];
  // the keys whose side the code tells
  const SIDED = { Shift: 1, Control: 1, Alt: 1, AltGraph: 1, Meta: 1, OS: 1 };

  // The one code point of a key, or 0 for a named key ('Enter', 'Dead')
  function codePointOf(key) {
    if (typeof key !== 'string' || !key) return 0;
    const c = key.codePointAt(0);
    return key.length === (c > 0xffff ? 2 : 1) ? c : 0;
  }
  const isControl = c => c < 32 || (c >= 0x7f && c < 0xa0);
  const isLetter = ch => /\p{L}/u.test(ch);
  const lower = ch => {
    const l = ch.toLowerCase();
    return codePointOf(l) ? l : ch;
  };

  // The system of the browser, by its platform: 'mac' (macOS and iOS),
  // 'windows' or 'other'
  function platform(nav) {
    nav = nav || (typeof navigator !== 'undefined' ? navigator : {});
    const name = (nav.userAgentData && nav.userAgentData.platform) || nav.platform || '';
    return /mac|iphone|ipad|ipod/i.test(name) ? 'mac' : /^win/i.test(name) ? 'windows' : 'other';
  }
  const isMac = nav => platform(nav) === 'mac';
  // the os argument: a platform, or whether it is macOS
  const onMac = os => os === true || os === 'mac';

  // AltGr types the third character of a key.  Chrome and Firefox say so
  // in getModifierState('AltGraph').  But on Windows AltGr is also Ctrl+Alt,
  // which then types a character that the key has neither alone nor with
  // Shift ('@' on Q of a German layout), or a key a US keyboard lacks has.
  // Elsewhere Ctrl+Alt is no AltGr, whatever the key types with them ('é'
  // on the 2 of a French layout): a shortcut
  function altGraph(e, os) {
    if (e.getModifierState && e.getModifierState('AltGraph')) return true;
    if (os !== 'windows' || !e.ctrlKey || !e.altKey || e.metaKey) return false;
    const c = codePointOf(e.key), base = US[e.code];
    if (!c || isControl(c)) return false;
    return !base || (lower(e.key) !== base && !(e.shiftKey && e.key === US_SHIFTED[e.code]));
  }

  function modifiers(e, os) {
    const mac = onMac(os);
    let m = 0;
    if (e.shiftKey) m |= SHIFT;
    if (e.ctrlKey || (mac && e.metaKey)) m |= CTRL;
    if (e.altKey) m |= ALT;
    if (!mac && e.metaKey) m |= CMD;
    if (altGraph(e, os)) m &= ~(CTRL | ALT);
    return m;
  }

  // Ctrl or Cmd held (AltGr aside): the key is a command, not typing
  function isShortcut(e, os) {
    return (e.ctrlKey || e.metaKey) && !altGraph(e, os);
  }

  function scanCode(e) {
    return SCAN[e.code] || 0;
  }

  function keySym(e, os) {
    const scan = scanCode(e), key = e.key, mac = onMac(os);
    if (key in CHAR_KEYS) return e.code === 'NumpadEnter' ? scan | MASK : CHAR_KEYS[key];
    if (key in NAMED) return ((SIDED[key] && scan) || NAMED[key]) | MASK;
    const c = codePointOf(key);
    if (c && !isControl(c)) {
      const ch = String.fromCodePoint(c), base = US[e.code];
      if (/^Numpad/.test(e.code) && scan) return scan | MASK;
      // Option on macOS and AltGr type another character (Option+P is π,
      // AltGr+Q on a German layout @): the key is still P, or Q
      if (base && ((mac && e.altKey) || altGraph(e, os))) return base.codePointAt(0);
      if (isLetter(ch)) return lower(ch).codePointAt(0);
      if (e.shiftKey && base) return base.codePointAt(0);
      return c;
    }
    // a dead key, or a key the browser cannot name
    if (US[e.code]) return US[e.code].codePointAt(0);
    return scan ? scan | MASK : 0;
  }

  function textOf(e, os) {
    if (e.isComposing) return 0;
    const c = codePointOf(e.key);
    if (!c || isControl(c) || isShortcut(e, os)) return 0;
    return c;
  }

  function key(e, os) {
    const sym = keySym(e, os), scan = scanCode(e), shortcut = isShortcut(e, os);
    const composing = !!e.isComposing || e.key === 'Dead' || e.key === 'Process' || e.key === 'Unidentified';
    const letter = shortcut && sym < 128 ? String.fromCharCode(sym) : '';
    const clipboard = letter === 'c' || letter === 'x' || letter === 'v';
    const modifierOnly = e.key in SIDED;
    return {
      sym, scan, shortcut,
      mods: modifiers(e, os),
      text: textOf(e, os),
      prevent: !composing && !modifierOnly && !clipboard && (sym !== 0 || shortcut),
      paste: letter === 'v' && !e.altKey,
    };
  }

  // DOM buttons (1 left, 2 right, 4 middle) as SDL's held masks
  function buttons(b) {
    return (b & 1) | (b & 4 ? 2 : 0) | (b & 2 ? 4 : 0);
  }
  // DOM button numbers (0 left, 1 middle, 2 right) as SDL's, or 0
  function button(b) {
    return b >= 0 && b <= 2 ? b + 1 : 0;
  }

  return {
    key, keySym, scanCode, modifiers, isShortcut, platform, isMac, buttons, button,
    MASK, SHIFT, CTRL, ALT, CMD,
  };
});
