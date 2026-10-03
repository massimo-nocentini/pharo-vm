// keymap.test.mjs - unit tests of packaging/emscripten/web/keymap.js
//
// usage: node keymap.test.mjs
//
// Feeds keymap.js KeyboardEvent-like objects as Chrome and Firefox make
// them (US, German and French layouts; Linux, Windows and macOS; the os
// argument 'other', 'windows', or true for macOS) and checks
// the SDL keycodes, scancodes, modifier bits and text that the world image
// gets, which keys the page keeps from the browser, and the mouse button
// numbers.  Prints every check and their count, and exits with status 1 if
// any fails.  Lane 80 (tests/wasm/lanes/80-world-harness.sh) runs it.

import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const here = path.dirname(fileURLToPath(import.meta.url));
const K = require(path.join(here, '..', '..', 'packaging', 'emscripten', 'web', 'keymap.js'));
const { MASK } = K;

let failures = 0, passes = 0;
function check(name, f) {
  try {
    f();
    passes++;
    console.log('ok - ' + name);
  } catch (e) {
    failures++;
    console.log('not ok - ' + name + '\n  ' + String((e && e.message) || e));
  }
}
function eq(got, want, what) {
  if (JSON.stringify(got) !== JSON.stringify(want))
    throw new Error(`${what}: got ${JSON.stringify(got)}, want ${JSON.stringify(want)}`);
}

// A KeyboardEvent: key and code, then the modifiers held ('shift ctrl'),
// and the modifier states of getModifierState ('AltGraph')
function ev(key, code, held = '', states = '') {
  const has = w => held.split(/\s+/).includes(w);
  return {
    key, code, shiftKey: has('shift'), ctrlKey: has('ctrl'), altKey: has('alt'), metaKey: has('meta'),
    repeat: has('repeat'), isComposing: has('composing'),
    getModifierState: s => states.split(/\s+/).includes(s),
  };
}
// what the image gets: [sym, scan, mods, text]
const got = (e, os = 'other') => { const k = K.key(e, os); return [k.sym, k.scan, k.mods, k.text]; };
const ch = c => c.codePointAt(0);

check('a letter: its lower case character, scancode 4.., its text', () => {
  eq(got(ev('a', 'KeyA')), [ch('a'), 4, 0, ch('a')], 'a');
  eq(got(ev('z', 'KeyZ')), [ch('z'), 29, 0, ch('z')], 'z');
  eq(got(ev('A', 'KeyA', 'shift')), [ch('a'), 4, 1, ch('A')], 'Shift+A');
});

check('digits and punctuation: the unshifted character, the shifted text', () => {
  eq(got(ev('1', 'Digit1')), [ch('1'), 30, 0, ch('1')], '1');
  eq(got(ev('0', 'Digit0')), [ch('0'), 39, 0, ch('0')], '0');
  eq(got(ev('!', 'Digit1', 'shift')), [ch('1'), 30, 1, ch('!')], 'Shift+1');
  eq(got(ev('+', 'Equal', 'shift')), [ch('='), 46, 1, ch('+')], 'Shift+=');
  eq(got(ev('.', 'Period')), [ch('.'), 55, 0, ch('.')], '.');
  eq(got(ev(':', 'Semicolon', 'shift')), [ch(';'), 51, 1, ch(':')], 'Shift+;');
  eq(got(ev(' ', 'Space')), [32, 44, 0, 32], 'Space');
});

check('the layout decides the character: French AZERTY, German QWERTZ', () => {
  eq(got(ev('a', 'KeyQ')), [ch('a'), 20, 0, ch('a')], 'AZERTY a on the Q key');
  eq(got(ev('A', 'KeyQ', 'shift')), [ch('a'), 20, 1, ch('A')], 'AZERTY Shift+A');
  eq(got(ev('&', 'Digit1')), [ch('&'), 30, 0, ch('&')], 'AZERTY & on the 1 key');
  eq(got(ev('z', 'KeyY')), [ch('z'), 28, 0, ch('z')], 'QWERTZ z on the Y key');
  eq(got(ev('ö', 'Semicolon')), [ch('ö'), 51, 0, ch('ö')], 'QWERTZ ö');
  eq(got(ev('Ö', 'Semicolon', 'shift')), [ch('ö'), 51, 1, ch('Ö')], 'QWERTZ Shift+ö');
});

check('keys that type no character: scancode | 0x40000000', () => {
  eq(got(ev('ArrowLeft', 'ArrowLeft')), [0x40000050, 80, 0, 0], 'ArrowLeft');
  eq(got(ev('ArrowRight', 'ArrowRight')), [0x4000004f, 79, 0, 0], 'ArrowRight');
  eq(got(ev('ArrowUp', 'ArrowUp')), [0x40000052, 82, 0, 0], 'ArrowUp');
  eq(got(ev('ArrowDown', 'ArrowDown')), [0x40000051, 81, 0, 0], 'ArrowDown');
  eq(got(ev('Home', 'Home')), [74 | MASK, 74, 0, 0], 'Home');
  eq(got(ev('PageDown', 'PageDown')), [78 | MASK, 78, 0, 0], 'PageDown');
  eq(got(ev('F1', 'F1')), [58 | MASK, 58, 0, 0], 'F1');
  eq(got(ev('F12', 'F12')), [69 | MASK, 69, 0, 0], 'F12');
  eq(got(ev('F13', 'F13')), [104 | MASK, 104, 0, 0], 'F13');
  eq(got(ev('CapsLock', 'CapsLock')), [57 | MASK, 57, 0, 0], 'CapsLock');
});

check('Enter, Tab, Backspace, Escape and Delete are characters, and type no text', () => {
  eq(got(ev('Enter', 'Enter')), [13, 40, 0, 0], 'Enter');
  eq(got(ev('Tab', 'Tab')), [9, 43, 0, 0], 'Tab');
  eq(got(ev('Tab', 'Tab', 'shift')), [9, 43, 1, 0], 'Shift+Tab');
  eq(got(ev('Backspace', 'Backspace')), [8, 42, 0, 0], 'Backspace');
  eq(got(ev('Escape', 'Escape')), [27, 41, 0, 0], 'Escape');
  eq(got(ev('Delete', 'Delete')), [127, 76, 0, 0], 'Delete');
});

check('the modifier keys: their side, and the bit they set', () => {
  eq(got(ev('Shift', 'ShiftLeft', 'shift')), [225 | MASK, 225, 1, 0], 'ShiftLeft');
  eq(got(ev('Shift', 'ShiftRight', 'shift')), [229 | MASK, 229, 1, 0], 'ShiftRight');
  eq(got(ev('Control', 'ControlLeft', 'ctrl')), [224 | MASK, 224, 2, 0], 'ControlLeft');
  eq(got(ev('Control', 'ControlRight', 'ctrl')), [228 | MASK, 228, 2, 0], 'ControlRight');
  eq(got(ev('Alt', 'AltLeft', 'alt')), [226 | MASK, 226, 4, 0], 'AltLeft');
  eq(got(ev('Meta', 'MetaLeft', 'meta')), [227 | MASK, 227, 8, 0], 'MetaLeft');
  eq(got(ev('Meta', 'MetaRight', 'meta'), true), [231 | MASK, 231, 2, 0], 'MetaRight on macOS');
  eq(got(ev('Shift', '', 'shift')), [225 | MASK, 0, 1, 0], 'a Shift of no known code');
});

check('the numeric keypad: SDL keypad keycodes, the text of the key', () => {
  eq(got(ev('1', 'Numpad1')), [89 | MASK, 89, 0, ch('1')], 'Numpad1');
  eq(got(ev('0', 'Numpad0')), [98 | MASK, 98, 0, ch('0')], 'Numpad0');
  eq(got(ev('+', 'NumpadAdd')), [87 | MASK, 87, 0, ch('+')], 'NumpadAdd');
  eq(got(ev('Enter', 'NumpadEnter')), [88 | MASK, 88, 0, 0], 'NumpadEnter');
  eq(got(ev('ArrowLeft', 'Numpad4')), [80 | MASK, 92, 0, 0], 'Numpad4 without NumLock');
});

check('shortcuts: Ctrl+letter is the letter with ctrl, and types nothing', () => {
  eq(got(ev('p', 'KeyP', 'ctrl')), [ch('p'), 19, 2, 0], 'Ctrl+P');
  eq(got(ev('P', 'KeyP', 'ctrl shift')), [ch('p'), 19, 3, 0], 'Ctrl+Shift+P');
  eq(got(ev('.', 'Period', 'alt')), [ch('.'), 55, 4, ch('.')], 'Alt+. (the interrupt key)');
  eq(got(ev('.', 'Period', 'ctrl')), [ch('.'), 55, 2, 0], 'Ctrl+.');
  eq(got(ev('a', 'KeyA', 'meta')), [ch('a'), 4, 8, 0], 'Meta+A off macOS: cmd');
});

check('macOS: Cmd is sent as Ctrl; Option types its character on the key\'s keycode', () => {
  eq(got(ev('p', 'KeyP', 'meta'), true), [ch('p'), 19, 2, 0], 'Cmd+P');
  eq(got(ev('p', 'KeyP', 'ctrl'), true), [ch('p'), 19, 2, 0], 'Ctrl+P');
  eq(got(ev('π', 'KeyP', 'alt'), true), [ch('p'), 19, 4, ch('π')], 'Option+P');
  eq(got(ev('@', 'KeyG', 'alt'), true), [ch('g'), 10, 4, ch('@')], 'Option+G on a layout typing @');
  eq(got(ev('.', 'Period', 'meta'), true), [ch('.'), 55, 2, 0], 'Cmd+.');
});

check('AltGr types its character, and is neither Ctrl nor Alt', () => {
  // Linux and Firefox: the AltGraph modifier state
  eq(got(ev('@', 'KeyQ', '', 'AltGraph')), [ch('q'), 20, 0, ch('@')], 'AltGr+Q (German), AltGraph state');
  eq(got(ev('{', 'Digit7', '', 'AltGraph')), [ch('7'), 36, 0, ch('{')], 'AltGr+7 (German)');
  eq(got(ev('@', 'KeyQ', 'ctrl alt', 'AltGraph'), 'windows'), [ch('q'), 20, 0, ch('@')], 'AltGr+Q (German), Windows, AltGraph state');
  // Windows: AltGr is Ctrl+Alt
  eq(got(ev('@', 'KeyQ', 'ctrl alt'), 'windows'), [ch('q'), 20, 0, ch('@')], 'AltGr+Q (German), as Ctrl+Alt');
  eq(got(ev('€', 'KeyE', 'ctrl alt'), 'windows'), [ch('e'), 8, 0, ch('€')], 'AltGr+E (German), as Ctrl+Alt');
  eq(got(ev('|', 'IntlBackslash', 'ctrl alt shift'), 'windows'), [ch('|'), 100, 1, ch('|')], 'AltGr+Shift on a key of no US character');
  // but Ctrl+Alt+a key that types its own character is a shortcut
  eq(got(ev('t', 'KeyT', 'ctrl alt'), 'windows'), [ch('t'), 23, 6, 0], 'Ctrl+Alt+T');
  eq(got(ev('@', 'KeyQ', 'ctrl alt'), true), [ch('q'), 20, 6, 0], 'Ctrl+Option+Q on macOS');
});

check('Ctrl+Alt is a shortcut when Shift explains the character, and off Windows', () => {
  // Windows: what the key types with Shift alone is no AltGr
  eq(got(ev('!', 'Digit1', 'ctrl alt shift'), 'windows'), [ch('1'), 30, 7, 0], 'Ctrl+Alt+Shift+1 (US)');
  eq(got(ev('?', 'Slash', 'ctrl alt shift'), 'windows'), [ch('/'), 56, 7, 0], 'Ctrl+Alt+Shift+/ (US)');
  eq(got(ev('T', 'KeyT', 'ctrl alt shift'), 'windows'), [ch('t'), 23, 7, 0], 'Ctrl+Alt+Shift+T');
  // but a character the key has with neither is AltGr, with Shift too
  eq(got(ev('@', 'Digit1', 'ctrl alt shift'), 'windows'), [ch('1'), 30, 1, ch('@')], 'AltGr+Shift+1 typing @');
  // Linux and ChromeOS: AltGr is a key of its own (the AltGraph state), and
  // Ctrl+Alt types nothing
  eq(got(ev('!', 'Digit1', 'ctrl alt shift')), [ch('1'), 30, 7, 0], 'Ctrl+Alt+Shift+1 (US)');
  eq(got(ev('é', 'Digit2', 'ctrl alt')), [ch('é'), 31, 6, 0], 'Ctrl+Alt+2 (French), a shortcut');
  eq(K.key(ev('!', 'Digit1', 'ctrl alt shift'), 'windows').shortcut, true, 'Ctrl+Alt+Shift+1 is a shortcut');
});

check('code points beyond the BMP, and keys of a composition', () => {
  eq(got(ev('😀', '')), [0x1f600, 0, 0, 0x1f600], 'an emoji');
  eq(got(ev('Dead', 'BracketLeft')), [ch('['), 47, 0, 0], 'a dead key');
  eq(got(ev('é', 'KeyE', 'composing')), [ch('é'), 8, 0, 0], 'a key of a composition types nothing');
  eq(got(ev('Unidentified', '')), [0, 0, 0, 0], 'an unidentified key');
  eq(got(ev('ab', 'KeyA')), [ch('a'), 4, 0, 0], 'a key of two characters types nothing');
});

check('what the page keeps from the browser', () => {
  const prevent = (e, mac) => K.key(e, mac).prevent;
  for (const [key, code] of [['Tab', 'Tab'], ['Backspace', 'Backspace'], ['ArrowLeft', 'ArrowLeft'], ['Escape', 'Escape'],
                             ['F5', 'F5'], ['Enter', 'Enter'], [' ', 'Space'], ['a', 'KeyA'], ['PageDown', 'PageDown']])
    eq(prevent(ev(key, code)), true, key);
  for (const letter of ['p', 'd', 'a', 's', 'w', 'f', 'z'])
    eq(prevent(ev(letter, 'Key' + letter.toUpperCase(), 'ctrl')), true, 'Ctrl+' + letter);
  eq(prevent(ev('p', 'KeyP', 'meta'), true), true, 'Cmd+P on macOS');
  for (const letter of ['c', 'x', 'v']) {
    eq(prevent(ev(letter, 'Key' + letter.toUpperCase(), 'ctrl')), false, 'Ctrl+' + letter);
    eq(prevent(ev(letter, 'Key' + letter.toUpperCase(), 'meta'), true), false, 'Cmd+' + letter + ' on macOS');
  }
  eq(prevent(ev('V', 'KeyV', 'ctrl shift')), false, 'Ctrl+Shift+V');
  eq(prevent(ev('Shift', 'ShiftLeft', 'shift')), false, 'Shift alone');
  eq(prevent(ev('Control', 'ControlLeft', 'ctrl')), false, 'Control alone');
  eq(prevent(ev('Dead', 'BracketLeft')), false, 'a dead key');
  eq(prevent(ev('é', 'KeyE', 'composing')), false, 'composing');
  eq(prevent(ev('Unidentified', '')), false, 'unidentified');
});

check('paste: Ctrl+V and Cmd+V, with Shift too, not Alt', () => {
  const paste = (e, mac) => K.key(e, mac).paste;
  eq(paste(ev('v', 'KeyV', 'ctrl')), true, 'Ctrl+V');
  eq(paste(ev('V', 'KeyV', 'ctrl shift')), true, 'Ctrl+Shift+V');
  eq(paste(ev('v', 'KeyV', 'meta'), true), true, 'Cmd+V on macOS');
  eq(paste(ev('v', 'KeyV')), false, 'v');
  eq(paste(ev('v', 'KeyV', 'ctrl alt')), false, 'Ctrl+Alt+V');
  eq(paste(ev('c', 'KeyC', 'ctrl')), false, 'Ctrl+C');
});

check('mouse buttons: SDL numbers and masks', () => {
  eq([0, 1, 2, 3, 4].map(K.button), [1, 2, 3, 0, 0], 'DOM 0 1 2 (3 4) as SDL 1 2 3 (none)');
  eq([0, 1, 2, 4, 3, 7].map(K.buttons), [0, 1, 4, 2, 5, 7], 'DOM masks (1 left, 2 right, 4 middle)');
});

check('platform and isMac: macOS and iOS, Windows, the others', () => {
  eq(K.isMac({ platform: 'MacIntel' }), true, 'MacIntel');
  eq(K.isMac({ platform: 'iPhone' }), true, 'iPhone');
  eq(K.isMac({ userAgentData: { platform: 'macOS' } }), true, 'userAgentData macOS');
  eq(K.isMac({ platform: 'Linux x86_64' }), false, 'Linux');
  eq(K.isMac({ platform: 'Win32' }), false, 'Win32');
  eq(K.isMac({}), false, 'none');
  eq(K.platform({ platform: 'MacIntel' }), 'mac', 'MacIntel');
  eq(K.platform({ platform: 'Win32' }), 'windows', 'Win32');
  eq(K.platform({ userAgentData: { platform: 'Windows' }, platform: 'Win32' }), 'windows', 'userAgentData Windows');
  eq(K.platform({ platform: 'Linux x86_64' }), 'other', 'Linux');
  eq(K.platform({ userAgentData: { platform: 'Chrome OS' } }), 'other', 'Chrome OS');
  eq(K.platform({}), 'other', 'none');
  // the boolean of isMac is a platform too
  eq(got(ev('p', 'KeyP', 'meta'), true), got(ev('p', 'KeyP', 'meta'), 'mac'), 'true is mac');
  eq(got(ev('p', 'KeyP', 'meta'), false), got(ev('p', 'KeyP', 'meta'), 'other'), 'false is other');
});

console.log(`# ${passes + failures} checks, ${passes} passed, ${failures} failed`);
process.exit(failures ? 1 : 0);
