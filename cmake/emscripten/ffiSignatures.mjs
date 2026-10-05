// ffiSignatures.mjs - the WebAssembly signatures of the rows of an FFI table
//
//   <clang -fsyntax-only -Xclang -ast-dump table.c> |
//   node ffiSignatures.mjs <table.o> <names>
//
// cmake/emscripten/genFFILibrary.cmake runs it at build time, on the table
// of a library of the registry (include/pharovm/emscripten/ffiRegistry.h)
// compiled with -c, and on the text dump of clang's AST of the same
// translation unit, on stdin.  <names> holds the names of the table's rows,
// one per line.  It prints a line '<name> <signature>' for each, in the
// order of <names>, where <signature> is
//
//   <result><parameters>   for a function: one character per WebAssembly
//                          value type, i (i32), j (i64), f (f32), d (f64),
//                          the result v when there is none, as the
//                          declaration of the headers lowers the function
//                          (a struct result is a first parameter j, the
//                          pointer to it, and the result v)
//   <...>.<n>              for a variadic function: its signature, whose
//                          last parameter is the pointer to the variable
//                          arguments, then the number n of its fixed
//                          arguments in C (sprintf: ijjj.2)
//   -                      for data
//
// src/emscripten/ffiAdapt.c reads the signatures, to adapt the callouts
// whose declaration differs from the function's in width.
//
// The signatures come from the object.  The table takes the address of
// every function of the library, which it does not define: in a relocatable
// object each one is then an undefined function symbol of the symbol table
// (the 'linking' section), whose import (the import section) has the exact
// type of the declaration (the type section).  A function that the headers
// define (static inline) is a defined function symbol, whose type is in the
// function section.  A row whose name is the C name of a symbol of another
// name (__asm__("label"), which the AST gives) takes the label's.  The
// variadic functions, and their number of fixed arguments, come from the
// AST: the FunctionDecl lines whose type has '...' as its last parameter.
// A row that is neither a function nor data of the object, a variadic
// function whose signature has no parameter for the variable arguments, and
// an object or a dump that this script cannot read stop it with an error:
// a change of the format of either breaks the build, rather than the
// signatures.

import { readFileSync } from 'node:fs';
import { createInterface } from 'node:readline';

const fail = (message) => {
  process.stderr.write(`ffiSignatures.mjs: ${message}\n`);
  process.exit(1);
};

if (process.argv.length != 4) fail('usage: node ffiSignatures.mjs <table.o> <names> < <AST dump>');
const [objectFile, namesFile] = process.argv.slice(2);

// --- The object: its function symbols with their signatures, and its data symbols

const VALUE_TYPES = { 0x7f: 'i', 0x7e: 'j', 0x7d: 'f', 0x7c: 'd' };
const SYMBOL_FUNCTION = 0, SYMBOL_DATA = 1, SYMBOL_GLOBAL = 2, SYMBOL_SECTION = 3, SYMBOL_TAG = 4,
      SYMBOL_TABLE = 5;
const FLAG_UNDEFINED = 0x10, FLAG_EXPLICIT_NAME = 0x40;
const LINKING_SYMBOL_TABLE = 8;

const readObject = (file) => {
  const bytes = readFileSync(file);
  let at = 0;
  const end = (what) => fail(`${file}: truncated ${what}`);
  const byte = () => (at < bytes.length ? bytes[at++] : end('byte'));
  // An unsigned LEB128, at most 32 bits significant (counts and indices)
  const u32 = () => {
    let value = 0, shift = 0, b;
    do {
      b = byte();
      if (shift < 32) value += (b & 0x7f) * 2 ** shift;
      shift += 7;
    } while (b & 0x80);
    return value;
  };
  const skipLeb = () => { while (byte() & 0x80); };
  const name = () => {
    const length = u32();
    if (at + length > bytes.length) end('name');
    const text = bytes.toString('utf8', at, at + length);
    at += length;
    return text;
  };
  const limits = () => {
    const flags = byte();
    skipLeb();
    if (flags & 1) skipLeb();
  };
  // The character of a value type, '?' for those that are not numbers
  const valueType = () => {
    const type = byte();
    if (type == 0x63 || type == 0x64) skipLeb(); // (ref null? <heap type>)
    return VALUE_TYPES[type] || '?';
  };

  if (bytes.length < 8 || bytes.readUInt32LE(0) != 0x6d736100 || bytes.readUInt32LE(4) != 1)
    fail(`${file} is not a WebAssembly module`);
  at = 8;
  const types = [];
  const functions = []; // the type of each function, imported ones first
  const importNames = []; // the field of each imported function
  let symbols = null;
  while (at < bytes.length) {
    const id = byte();
    const size = u32();
    const sectionEnd = at + size;
    if (sectionEnd > bytes.length) end('section');
    if (id == 1) { // types
      for (let n = u32(); n > 0; n--) {
        const form = byte();
        if (form != 0x60) fail(`${file}: a type of form 0x${form.toString(16)}, not a function type`);
        const parameters = [];
        for (let k = u32(); k > 0; k--) parameters.push(valueType());
        const results = [];
        for (let k = u32(); k > 0; k--) results.push(valueType());
        types.push(results.length > 1 ? null : (results[0] || 'v') + parameters.join(''));
      }
    } else if (id == 2) { // imports
      for (let n = u32(); n > 0; n--) {
        name();
        const field = name();
        const kind = byte();
        if (kind == 0) { importNames.push(field); functions.push(types[u32()]); }
        else if (kind == 1) { valueType(); limits(); }
        else if (kind == 2) limits();
        else if (kind == 3) { valueType(); byte(); }
        else if (kind == 4) { byte(); u32(); }
        else fail(`${file}: an import of kind ${kind}`);
      }
    } else if (id == 3) { // functions defined
      for (let n = u32(); n > 0; n--) functions.push(types[u32()]);
    } else if (id == 0) { // custom
      const section = name();
      if (section == 'linking') {
        const version = u32();
        if (version != 2) fail(`${file}: version ${version} of the linking section, not 2`);
        while (at < sectionEnd) {
          const type = byte();
          const length = u32();
          const subsectionEnd = at + length;
          if (type == LINKING_SYMBOL_TABLE) {
            symbols = [];
            for (let n = u32(); n > 0; n--) {
              const kind = byte();
              const flags = u32();
              const symbol = { kind, flags, name: null, index: -1 };
              if (kind == SYMBOL_DATA) {
                symbol.name = name();
                if (!(flags & FLAG_UNDEFINED)) { u32(); skipLeb(); skipLeb(); }
              } else if (kind == SYMBOL_SECTION) {
                u32();
              } else if (kind == SYMBOL_FUNCTION || kind == SYMBOL_GLOBAL || kind == SYMBOL_TAG
                         || kind == SYMBOL_TABLE) {
                symbol.index = u32();
                if (!(flags & FLAG_UNDEFINED) || (flags & FLAG_EXPLICIT_NAME)) symbol.name = name();
              } else fail(`${file}: a symbol of kind ${kind}`);
              symbols.push(symbol);
            }
          }
          at = subsectionEnd;
        }
      }
    }
    at = sectionEnd;
  }
  if (!symbols) fail(`${file} has no symbol table: it is not a relocatable object (compile it with -c)`);

  const signatures = new Map();
  const data = new Set();
  for (const symbol of symbols)
    if (symbol.kind == SYMBOL_FUNCTION) {
      // An undefined function without an explicit name has its import's
      const symbolName = symbol.name ?? importNames[symbol.index];
      const type = functions[symbol.index];
      if (symbolName === undefined || type === undefined)
        fail(`${file}: the function symbol of index ${symbol.index} has no import or function`);
      if (type === null || type.includes('?'))
        fail(`${file}: ${symbolName} has a type that is not of i32, i64, f32 and f64 with at most one result`);
      signatures.set(symbolName, type);
    } else if (symbol.kind == SYMBOL_DATA) data.add(symbol.name);
  return { signatures, data };
};

// --- The AST: the variadic functions, their numbers of fixed arguments, and asm labels

// The text between the parenthesis at open and its match
const closing = (text, open) => {
  for (let depth = 0, i = open; i < text.length; i++)
    if (text[i] == '(' || text[i] == '[') depth++;
    else if ((text[i] == ')' || text[i] == ']') && --depth == 0) return i;
  return -1;
};
const opening = (text, close) => {
  for (let depth = 0, i = close; i >= 0; i--)
    if (text[i] == ')' || text[i] == ']') depth++;
    else if ((text[i] == '(' || text[i] == '[') && --depth == 0) return i;
  return -1;
};

// The parameters of a function of type type, as clang prints it: the list
// in the last parentheses, but where it returns a pointer to a function or
// an array, 'void (*(int, void (*)(int)))(int)' (signal), the list of the
// declarator inside.  null when type is not one of a function.
const parametersOf = (type) => {
  let text = type.trim();
  for (;;) { // __attribute__((noreturn)) and its kind follow the list
    const attribute = text.match(/\s+__attribute__\(\((?:[^()]|\([^()]*\))*\)\)$/);
    if (!attribute) break;
    text = text.slice(0, attribute.index).trimEnd();
  }
  while (text.endsWith(']')) { // the array of a pointer to an array
    const open = opening(text, text.length - 1);
    if (open < 0) return null;
    text = text.slice(0, open).trimEnd();
    if (!text.endsWith(')')) return null;
    const inner = opening(text, text.length - 1);
    text = text.slice(inner + 1, -1).replace(/^[\s*^&]*(?:(?:const|volatile|restrict|_Nonnull|_Nullable|_Null_unspecified)\b[\s*^&]*)*/, '');
  }
  if (!text.endsWith(')')) return null;
  const open = opening(text, text.length - 1);
  if (open < 0) return null;
  const before = text.slice(0, open).trimEnd();
  if (before.endsWith(')')) { // a function returning a pointer to a function
    const inner = opening(before, before.length - 1);
    if (inner < 0) return null;
    return parametersOf(before.slice(inner + 1, -1)
      .replace(/^[\s*^&]*(?:(?:const|volatile|restrict|_Nonnull|_Nullable|_Null_unspecified)\b[\s*^&]*)*/, ''));
  }
  const list = text.slice(open + 1, -1);
  const parameters = [];
  let depth = 0, start = 0;
  for (let i = 0; i < list.length; i++)
    if (list[i] == '(' || list[i] == '[') depth++;
    else if (list[i] == ')' || list[i] == ']') depth--;
    else if (list[i] == ',' && depth == 0) { parameters.push(list.slice(start, i).trim()); start = i + 1; }
  if (list.trim()) parameters.push(list.slice(start).trim());
  if (parameters.length == 1 && parameters[0] == 'void') parameters.length = 0;
  return parameters;
};

// FunctionDecl lines at the top of the translation unit, as
// '|-FunctionDecl 0x... [prev 0x...] <range> col:N [implicit] [used] name 'type'[:'desugared'] ...',
// and their AsmLabelAttr children, '| `-AsmLabelAttr 0x... <col:N> "label"'
const readAst = async () => {
  const functions = new Map();
  let current = null, lines = 0;
  for await (const line of createInterface({ input: process.stdin, crlfDelay: Infinity })) {
    lines++;
    if (/^[|`]-/.test(line)) {
      current = null;
      if (!line.startsWith('FunctionDecl ', 2)) continue;
      const match = line.match(/ ([A-Za-z_][A-Za-z0-9_]*) '([^']*)'(?::'([^']*)')?/);
      if (!match) fail(`cannot read the AST line ${line}`);
      const [, name, type, desugared] = match;
      const parameters = parametersOf(desugared ?? type);
      if (!parameters) fail(`cannot read the parameters of ${name}, of type ${desugared ?? type}`);
      const variadic = parameters.length > 0 && parameters[parameters.length - 1] == '...';
      const previous = functions.get(name);
      current = { variadic, fixed: parameters.length - (variadic ? 1 : 0), label: previous?.label ?? null };
      functions.set(name, current);
    } else if (current && /^[| ] [|`]-AsmLabelAttr /.test(line)) {
      const match = line.match(/"([^"]*)"\s*$/);
      if (match) current.label = match[1];
    }
  }
  if (lines == 0) fail('the AST dump on stdin is empty');
  return functions;
};

// --- The rows

const { signatures, data } = readObject(objectFile);
const ast = await readAst();
const names = readFileSync(namesFile, 'utf8').split('\n').map((line) => line.trim()).filter((line) => line);
const output = [];
for (const name of names) {
  const declaration = ast.get(name);
  const symbol = declaration?.label ?? name;
  if (signatures.has(symbol)) {
    let signature = signatures.get(symbol);
    if (declaration?.variadic) {
      // The fixed arguments, then the pointer to the variable ones
      if (signature.length - 1 < declaration.fixed + 1)
        fail(`${name} is variadic with ${declaration.fixed} fixed arguments, but its signature ${signature} `
             + 'has no parameter for the variable arguments');
      signature += `.${declaration.fixed}`;
    }
    output.push(`${name} ${signature}`);
  } else if (data.has(symbol)) {
    output.push(`${name} -`);
  } else {
    fail(`${name}${symbol != name ? ` (${symbol})` : ''} is neither a function nor data of ${objectFile}`);
  }
}
process.stdout.write(output.join('\n') + (output.length ? '\n' : ''));
