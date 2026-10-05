// prim-audit.mjs - audit the builtin primitive tables of the node CLI
//
// The interpreter calls every named primitive through void (*)(void), and a
// WebAssembly indirect call traps unless its callee has exactly that type.
// The build therefore gives every exports table a companion table of void
// trampolines (cmake/emscripten/genPrimitiveTable.cmake), which
// src/common/sqNamedPrims.c uses for named-primitive lookups.  This script
// checks the result in the linked module.  It loads pharo.js without running
// the VM, lists pluginPrimitives through vm_list_builtin_primitives() and
// what the lookup of sqNamedPrims.c answers for each row through
// vm_list_builtin_lookups(), finds each function through the element section
// of pharo.wasm, reads which rows carry an accessor depth (the primitives) in
// the exports tables of the sources the trampolines were generated from, and
// requires that
//
//   - every row's function is in the table, at an index of at least 1024
//     (-sTABLE_BASE=1024: no function pointer looks like a quick primitive);
//   - a named-primitive lookup (with an accessor depth) answers the row's
//     function in pluginPrimitives and that depth, and a function lookup
//     (without one) its function in pluginExports;
//   - every row that carries an accessor depth in its source, and every row
//     that goes through a trampoline, points to a () -> () function, and
//     every row that goes through a trampoline carries an accessor depth in
//     its source (so the depths were read);
//   - every other row that keeps its typed function is () -> () or is not a
//     primitive (its name does not start with 'prim'), for tables written by
//     hand;
//   - no function is wrapped by -sEMULATE_FUNCTION_POINTER_CASTS;
//   - the builtin modules are the in-tree plugins linked into the VM (with
//     UUIDPlugin when the CMake cache of the build turns FEATURE_PLUGIN_UUID
//     on) plus one for each src/emscripten/plugins/*.c, and every row with an
//     accessor depth in their sources is in the tables;
//   - when the CMake cache turns FEATURE_FFI on, the module-less table
//     vmsupport_exports is the one genSupportTable.cmake wrote into the
//     trampolines' directory, and it lists exactly the support primitives of
//     the same-thread FFI, every Primitive() and PrimitiveWithDepth() of
//     src/ffi but those of src/ffi/worker (the threaded FFI), each with its
//     accessor depth: then the lookups find them as dlsym does natively.
//     Without the FFI, none of them is in the tables but the stand-in
//     primitiveInitilizeCallbacks of src/emscripten/emscriptenSupport.c.
//
// Usage: node prim-audit.mjs [--srcdir <dir>] [--prims <dir>] <build-wasm/node/pharo.js>
// where the source tree <dir> defaults to the one holding this script, and
// --prims, the generated trampolines (<X>_primitives.c), to cmake/wasm/prims
// of the build directory holding node/pharo.js.  Prints what it checked and
// exits 0, or prints the failures and exits 1.

import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { createRequire } from 'node:module';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

const TABLE_BASE = 1024;

// The in-tree plugins cmake/Emscripten.cmake links into the VM.
const IN_TREE_PLUGINS = [
    'B2DPlugin', 'BitBltPlugin', 'DSAPrims', 'FileAttributesPlugin', 'FilePlugin',
    'FloatArrayPlugin', 'JPEGReaderPlugin', 'JPEGReadWriter2Plugin', 'LargeIntegers',
    'LocalePlugin', 'MiscPrimitivePlugin', 'NewFilePlugin', 'SocketPlugin',
];

const ENGINE_ERRORS = /RuntimeError|RangeError|Aborted|unreachable|signature_mismatch|unsupported syscall/;

const thisFile = fileURLToPath(import.meta.url);

// counted(1, 'row') is '1 row', counted(2, 'row') '2 rows'.
const counted = (n, one, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

// The child: instantiate the module with its own glue, but without calling
// main, and print the table rows.
function dump(js) {
    const Module = {
        noInitialRun: true,
        onRuntimeInitialized() {
            if (typeof Module._vm_list_builtin_primitives !== 'function') {
                console.error(`${js} does not export vm_list_builtin_primitives`);
                process.exit(2);
            }
            Module._vm_list_builtin_primitives();
            if (typeof Module._vm_list_builtin_lookups === 'function')
                Module._vm_list_builtin_lookups();
            process.exit(0);
        },
    };
    // The glue is a CommonJS script that picks up a Module in scope.
    const run = vm.runInThisContext(
        '(function (require, module, exports, __filename, __dirname, Module) {'
            + readFileSync(js, 'utf8') + '\n})',
        { filename: js });
    const module = { exports: {} };
    run(createRequire(js), module, module.exports, js, dirname(js), Module);
}

class Reader {
    constructor(bytes, start = 0, end = bytes.length) {
        this.bytes = bytes;
        this.pos = start;
        this.end = end;
    }

    u8() {
        if (this.pos >= this.end)
            throw new Error('truncated section');
        return this.bytes[this.pos++];
    }

    // LEB128, exact up to 2^53, which is plenty for indices and offsets.
    leb(signed = false) {
        let result = 0, scale = 1, byte;
        do {
            byte = this.u8();
            result += (byte & 0x7f) * scale;
            scale *= 128;
        } while (byte & 0x80);
        return signed && (byte & 0x40) ? result - scale : result;
    }

    name() {
        const length = this.leb();
        const text = new TextDecoder().decode(this.bytes.subarray(this.pos, this.pos + length));
        this.pos += length;
        return text;
    }

    vector(read) {
        const items = [];
        for (let n = this.leb(); n > 0; n--)
            items.push(read());
        return items;
    }

    valueType() {
        const type = this.u8();
        if (type === 0x63 || type === 0x64)     // (ref null? <heap type>)
            this.leb(true);
        return type;
    }

    limits() {
        const flags = this.u8();
        this.leb();
        if (flags & 0x01)
            this.leb();
        if (flags & 0x08)                       // custom page size
            this.leb();
    }

    // An i32.const or i64.const offset; NaN for anything else (global.get).
    constant() {
        const opcode = this.u8();
        const operand = this.leb(opcode !== 0x23);
        const value = opcode === 0x41 || opcode === 0x42 ? operand : NaN;
        if (this.u8() !== 0x0b)
            throw new Error('unsupported constant expression');
        return value;
    }

    // A function index from an element expression; undefined for ref.null.
    elementExpression() {
        const opcode = this.u8();
        const operand = this.leb(opcode === 0xd0);
        const index = opcode === 0xd2 ? operand : undefined;
        if (this.u8() !== 0x0b)
            throw new Error('unsupported element expression');
        return index;
    }
}

// The parts of the module the audit needs: the function types, the entries
// of table 0 and the function names.
function parseModule(bytes) {
    const magic = [0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    if (!magic.every((byte, i) => bytes[i] === byte))
        throw new Error('not a WebAssembly module');
    const types = [];
    const functionTypes = [];                   // function index -> type index
    const table = new Map();                    // table index -> function index
    const names = new Map();                    // function index -> name
    let hasNames = false;

    const r = new Reader(bytes, 8);
    while (r.pos < bytes.length) {
        const id = r.u8();
        const size = r.leb();
        const s = new Reader(bytes, r.pos, r.pos + size);
        r.pos += size;
        switch (id) {
        case 0:                                 // custom
            if (s.name() !== 'name' || hasNames)
                break;
            hasNames = true;
            while (s.pos < s.end) {
                const subsection = s.u8();
                const length = s.leb();
                const next = s.pos + length;
                if (subsection === 1)           // function names
                    s.vector(() => names.set(s.leb(), s.name()));
                s.pos = next;
            }
            break;
        case 1:                                 // type
            s.vector(() => {
                const form = s.u8();
                if (form !== 0x60)
                    throw new Error(`unsupported type form 0x${form.toString(16)}`);
                types.push({ params: s.vector(() => s.valueType()), results: s.vector(() => s.valueType()) });
            });
            break;
        case 2:                                 // import
            s.vector(() => {
                s.name();
                s.name();
                const kind = s.u8();
                if (kind === 0) {               // function
                    functionTypes.push(s.leb());
                } else if (kind === 1) {        // table
                    s.valueType();
                    s.limits();
                } else if (kind === 2) {        // memory
                    s.limits();
                } else if (kind === 3) {        // global
                    s.valueType();
                    s.u8();
                } else if (kind === 4) {        // tag
                    s.u8();
                    s.leb();
                } else {
                    throw new Error(`unsupported import kind ${kind}`);
                }
            });
            break;
        case 3:                                 // function
            s.vector(() => functionTypes.push(s.leb()));
            break;
        case 9:                                 // element
            s.vector(() => {
                const flags = s.leb();
                const active = (flags & 0x01) === 0;
                let tableIndex = 0, offset = NaN;
                if (active) {
                    if (flags & 0x02)
                        tableIndex = s.leb();
                    offset = s.constant();
                }
                if (flags & 0x03) {             // an element kind or a reference type
                    if (flags & 0x04)
                        s.valueType();
                    else
                        s.u8();
                }
                const entries = flags & 0x04
                    ? s.vector(() => s.elementExpression())
                    : s.vector(() => s.leb());
                if (active && tableIndex === 0 && !Number.isNaN(offset))
                    entries.forEach((f, i) => { if (f !== undefined) table.set(offset + i, f); });
            });
            break;
        }
    }
    return { types, functionTypes, table, names, hasNames };
}

// The rows that carry an accessor depth in the exports tables the build
// made trampolines for: Map '<module> <name>' -> {depth, conditional, table},
// module '-' for the VM's own rows, conditional when the row is inside an
// #if of its table.  Each <X>_primitives.c of prims #includes the source of
// <X>_exports; vmsupport_exports, which needs no trampolines, is in
// src/emscripten/emscriptenSupport.c, or with the FFI (ffi) in
// vmsupport_exports.c of prims.
function depthRows(prims, srcdir, ffi, failures) {
    const rows = new Map();
    if (!existsSync(prims) || !statSync(prims).isDirectory()) {
        failures.push(`${prims} does not hold the generated trampolines (<X>_primitives.c): pass --prims`);
        return rows;
    }
    const tables = [];
    const files = readdirSync(prims).filter((f) => f.endsWith('_primitives.c')).sort();
    if (files.length === 0)
        failures.push(`${prims} holds no <X>_primitives.c: pass --prims the generated trampolines`);
    for (const file of files) {
        const include = /^#include "(.*)"$/m.exec(readFileSync(join(prims, file), 'utf8'));
        if (include)
            tables.push({ source: include[1], table: file.replace(/_primitives\.c$/, '_exports') });
        else
            failures.push(`${join(prims, file)} includes no source`);
    }
    // (with the FFI, the table of the support primitives of src/ffi that
    // genSupportTable.cmake writes next to the trampolines)
    tables.push({ source: ffi ? join(prims, 'vmsupport_exports.c') : resolve(srcdir, 'src/emscripten/emscriptenSupport.c'),
        table: 'vmsupport_exports' });
    for (const { source, table } of tables) {
        const text = existsSync(source) ? readFileSync(source, 'utf8').replace(/\r\n/g, '\n') : '';
        const module = /^static char _m\[\] = "([^"]*)";$/m.exec(text);
        const header = new RegExp(`\\nvoid ?\\* ?${table}\\[\\]\\[3\\] = \\{\\n`).exec(text);
        const start = header ? header.index : -1;
        const end = start < 0 ? -1 : text.indexOf('\n};\n', start);
        if (!module || end < 0) {
            failures.push(`${source}: no module name, or no ${table} table`);
            continue;
        }
        let conditional = 0;
        for (const line of text.slice(start, end).split('\n').slice(2)) {
            if (/^#\s*if/.test(line))
                conditional++;
            else if (/^#\s*endif/.test(line))
                conditional--;
            const row = /^\t\{\(void\*\)_m, "([A-Za-z_]\w*)\\000\\([0-7]{3})", \(void\*\)\w+\},$/.exec(line);
            if (row) {
                const depth = parseInt(row[2], 8);
                rows.set(`${module[1] || '-'} ${row[1]}`,
                    { depth: depth > 127 ? depth - 256 : depth, conditional: conditional > 0, table });
            }
        }
    }
    return rows;
}

// The support primitives of the same-thread FFI: Map name -> accessor
// depth of every Primitive(name) and PrimitiveWithDepth(name, N) definition
// (include/pharovm/macros.h) of the sources of src/ffi, those of
// src/ffi/worker excepted, which genSupportTable.cmake lists.
function ffiPrimitives(srcdir, failures) {
    const primitives = new Map();
    const walk = (dir) => {
        for (const entry of readdirSync(dir).sort()) {
            const path = join(dir, entry);
            if (statSync(path).isDirectory()) {
                if (entry !== 'worker')
                    walk(path);
            } else if (entry.endsWith('.c')) {
                for (const line of readFileSync(path, 'utf8').split(/\r?\n/)) {
                    if (!/^[ \t]*Primitive(WithDepth)?[ \t]*\(/.test(line))
                        continue;
                    const m = /^[ \t]*Primitive(?:WithDepth[ \t]*\([ \t]*([A-Za-z_]\w*)[ \t]*,[ \t]*(-?\d+)|[ \t]*\([ \t]*([A-Za-z_]\w*))[ \t]*\)/.exec(line);
                    if (m)
                        primitives.set(m[1] ?? m[3], m[1] ? Number(m[2]) : 0);
                    else
                        failures.push(`${path}: cannot read the primitive definition '${line.trim()}'`);
                }
            }
        }
    };
    const dir = resolve(srcdir, 'src/ffi');
    if (existsSync(dir))
        walk(dir);
    if (primitives.size === 0)
        failures.push(`${dir} defines no support primitive of the FFI`);
    return primitives;
}

function signature(type) {
    const names = { 0x7f: 'i32', 0x7e: 'i64', 0x7d: 'f32', 0x7c: 'f64', 0x7b: 'v128', 0x70: 'funcref', 0x6f: 'externref' };
    const list = (types) => types.map((t) => names[t] ?? `0x${t.toString(16)}`).join(', ');
    return `(${list(type.params)}) -> (${list(type.results)})`;
}

function audit(js, srcdir, prims) {
    const wasm = js.replace(/\.js$/, '.wasm');
    for (const file of [js, wasm])
        if (!existsSync(file))
            return [`${file} does not exist`];
    const failures = [];

    const child = spawnSync(process.execPath, [thisFile, '--dump', js],
        { encoding: 'utf8', maxBuffer: 64 << 20, timeout: 120000 });
    if (child.error || child.status !== 0 || ENGINE_ERRORS.test(child.stderr)) {
        const stderr = child.stderr.trim().split('\n').slice(0, 4).map((l) => l.slice(0, 200)).join('\n');
        return [`listing the rows of ${js} failed (status ${child.status}): ${child.error ?? stderr}`];
    }
    // The rows: "<module> <name> <table index> <exports table index>", module
    // '-' for the VM's own rows; the two indices differ where a trampoline was
    // put.  Then what the lookup answers for the names of each row:
    // "lookup <module> <name> <primitive> <depth> <function>".
    const rows = [], lookups = [];
    for (const line of child.stdout.split('\n').filter((l) => l !== '')) {
        let m;
        if ((m = /^lookup (\S+) (\S+) (\d+) (-?\d+) (\d+)$/.exec(line)))
            lookups.push({ module: m[1], name: m[2], primitive: Number(m[3]), depth: Number(m[4]), function: Number(m[5]) });
        else if ((m = /^(\S+) (\S+) (\d+) (\d+)$/.exec(line)))
            rows.push({ module: m[1], name: m[2], index: Number(m[3]), exportIndex: Number(m[4]) });
        else
            return [`unexpected line in the listing of ${js}: '${line}'`];
    }
    if (rows.length === 0)
        return [`vm_list_builtin_primitives() listed no rows`];

    const { types, functionTypes, table, names, hasNames } = parseModule(readFileSync(wasm));
    if (!hasNames)
        failures.push(`${wasm} has no function names (the node CLI links with --profiling-funcs)`);
    const wrapped = [...names.values()].filter((n) => n.includes('byn$fpcast-emu$'));
    if (wrapped.length > 0)
        failures.push(`${counted(wrapped.length, 'function is', 'functions are')} wrapped by -sEMULATE_FUNCTION_POINTER_CASTS, e.g. ${wrapped[0]}`);

    const pluginDir = resolve(srcdir, 'src/emscripten/plugins');
    const expected = new Set(IN_TREE_PLUGINS);
    // (the CMake cache of the build holding the trampolines, cmake/wasm/prims)
    const cache = resolve(prims, '..', '..', 'CMakeCache.txt');
    const cacheText = existsSync(cache) ? readFileSync(cache, 'utf8') : '';
    if (/^FEATURE_PLUGIN_UUID:BOOL=(ON|TRUE|YES|Y|1)$/mi.test(cacheText))
        expected.add('UUIDPlugin');
    const ffi = /^FEATURE_FFI:BOOL=(ON|TRUE|YES|Y|1)$/mi.test(cacheText);
    if (existsSync(pluginDir))
        for (const file of readdirSync(pluginDir).filter((f) => f.endsWith('.c')))
            expected.add(basename(file, '.c'));
    const modules = new Set(rows.map((row) => row.module).filter((m) => m !== '-'));
    const missing = [...expected].filter((m) => !modules.has(m));
    const unexpected = [...modules].filter((m) => !expected.has(m));
    if (missing.length > 0)
        failures.push(`builtin modules missing: ${missing.join(' ')}`);
    if (unexpected.length > 0)
        failures.push(`unexpected builtin modules: ${unexpected.join(' ')}`);

    const functionAt = (index, where) => {
        if (index < TABLE_BASE)
            failures.push(`${where}: table index ${index} is below ${TABLE_BASE}`);
        const f = table.get(index);
        if (f === undefined)
            failures.push(`${where}: table index ${index} is not in the element section`);
        return f;
    };

    // The lookup finds the first row of a name; the lookups follow the rows
    // of pluginExports, which pluginPrimitives lists in the same order.
    const firstRow = new Map();
    for (const row of rows)
        if (!firstRow.has(`${row.module} ${row.name}`))
            firstRow.set(`${row.module} ${row.name}`, row);
    const depths = depthRows(prims, srcdir, ffi, failures);
    if (lookups.length !== rows.length)
        failures.push(`vm_list_builtin_lookups() listed ${counted(lookups.length, 'lookup')} for ${counted(rows.length, 'row')}`
            + (lookups.length === 0 ? ' (an older VM, or an emscriptenSupport.c without it)' : ''));
    for (const [i, lookup] of lookups.entries()) {
        const where = `${lookup.module} ${lookup.name}`;
        const row = firstRow.get(where);
        if (i >= rows.length || `${rows[i].module} ${rows[i].name}` !== where || !row) {
            failures.push(`lookup ${i} (${where}) is not row ${i} of pluginPrimitives: the tables do not match`);
            break;
        }
        if (lookup.primitive !== row.index)
            failures.push(`${where}: the named-primitive lookup answers table index ${lookup.primitive}, `
                + `not its function ${row.index} in pluginPrimitives`);
        if (lookup.function !== row.exportIndex)
            failures.push(`${where}: the function lookup answers table index ${lookup.function}, `
                + `not its function ${row.exportIndex} in pluginExports`);
        const source = depths.get(where);
        if (source && lookup.depth !== source.depth)
            failures.push(`${where}: the lookup answers accessor depth ${lookup.depth}, not ${source.depth} as in ${source.table}`);
    }

    // genPrimitiveTable.cmake puts a trampoline for exactly the rows that
    // carry an accessor depth, so a trampoline without a depth row means that
    // the depths of its table were not read (and the checks above and below
    // that need them would pass vacuously).
    let trampolines = 0, typed = 0, depthChecked = 0;
    const unread = new Map();                   // module -> trampolined rows without a depth row
    for (const row of rows) {
        const where = `${row.module} ${row.name}`;
        const f = functionAt(row.index, where);
        functionAt(row.exportIndex, `${where} (exports table)`);
        if (f === undefined)
            continue;
        const type = types[functionTypes[f]];
        const isVoid = type.params.length === 0 && type.results.length === 0;
        const name = names.get(f) ?? `function ${f}`;
        const hasDepth = depths.has(where);
        if (hasDepth)
            depthChecked++;
        if (row.index !== row.exportIndex) {
            trampolines++;
            if (!hasDepth)
                unread.set(row.module, [...(unread.get(row.module) ?? []), row.name]);
        }
        if (isVoid)
            continue;
        if (hasDepth)
            failures.push(`${where}: a primitive (it carries an accessor depth), and ${name} has type ${signature(type)}, not () -> ()`);
        else if (row.index !== row.exportIndex)
            failures.push(`${where}: its trampoline ${name} has type ${signature(type)}, not () -> ()`);
        else {
            typed++;
            if (row.name.startsWith('prim'))
                failures.push(`${where}: no trampoline, and ${name} has type ${signature(type)}, not () -> ()`);
        }
    }
    if (unread.size > 0) {
        const owner = (module) => (module === '-' ? 'the VM' : module);
        const [module, unreadRows] = [...unread][0];
        const count = [...unread.values()].reduce((sum, n) => sum + n.length, 0);
        failures.push(`cannot read the accessor depths of ${[...unread.keys()].map(owner).join(', ')}: `
            + `${counted(count, 'row goes', 'rows go')} through a trampoline, but the sources of ${prims} give `
            + `${count === 1 ? 'it' : 'them'} no accessor depth `
            + `(e.g. ${unreadRows[0]} of ${owner(module)})`);
    }
    // The support primitives of the FFI: with it, the table lists them all,
    // with their depths, and the lookups found them with those depths (above);
    // without it, none is in the tables but the stand-in.
    const support = ffiPrimitives(srcdir, failures);
    const vmRows = new Set(rows.filter((row) => row.module === '-').map((row) => row.name));
    if (ffi) {
        const table = new Map([...depths].filter(([, source]) => source.table === 'vmsupport_exports')
            .map(([where, source]) => [where.slice(2), source.depth]));
        for (const [name, depth] of support) {
            if (!table.has(name))
                failures.push(`- ${name}: a support primitive of src/ffi, but not in vmsupport_exports`);
            else if (table.get(name) !== depth)
                failures.push(`- ${name}: accessor depth ${table.get(name)} in vmsupport_exports, ${depth} in src/ffi`);
            if (!vmRows.has(name))
                failures.push(`- ${name}: a support primitive of src/ffi, but not in the builtin tables`);
        }
        for (const name of table.keys())
            if (!support.has(name))
                failures.push(`- ${name}: in vmsupport_exports, but not a support primitive of src/ffi`);
    } else {
        for (const name of support.keys())
            if (name !== 'primitiveInitilizeCallbacks' && vmRows.has(name))
                failures.push(`- ${name}: a support primitive of src/ffi in the tables, but the build has no FFI`);
    }

    const listed = new Set([...modules, '-']);
    for (const [where, source] of depths)
        if (!source.conditional && listed.has(where.split(' ')[0]) && !firstRow.has(where))
            failures.push(`${where}: carries an accessor depth in ${source.table}, but is not in the builtin tables`);

    if (failures.length === 0)
        console.log(`prim-audit: ${basename(wasm)}: ${rows.length} rows in ${modules.size} modules, `
            + `${trampolines} through void trampolines, ${depthChecked} primitives with an accessor depth `
            + `all () -> (), ${typed} typed exports, ${lookups.length} lookups as listed, `
            + `every table index >= ${TABLE_BASE}, no fpcast-emu wrappers, `
            + (ffi ? `the ${support.size} support primitives of src/ffi` : 'no FFI'));
    return failures;
}

function main(args) {
    let srcdir = resolve(dirname(thisFile), '../..');
    let prims = null;
    const files = [];
    for (let i = 0; i < args.length; i++) {
        if (args[i] === '--dump')
            return dump(resolve(args[++i]));
        if (args[i] === '--srcdir')
            srcdir = resolve(args[++i]);
        else if (args[i] === '--prims')
            prims = resolve(args[++i]);
        else
            files.push(resolve(args[i]));
    }
    if (files.length !== 1) {
        console.error('usage: node prim-audit.mjs [--srcdir <dir>] [--prims <dir>] <pharo.js>');
        process.exit(2);
    }
    const failures = audit(files[0], srcdir, prims ?? resolve(dirname(files[0]), '../cmake/wasm/prims'));
    const shown = 25;
    for (const failure of failures.slice(0, shown))
        console.log(`FAIL prim-audit: ${failure}`);
    if (failures.length > shown)
        console.log(`FAIL prim-audit: ... and ${failures.length - shown} more`);
    process.exitCode = failures.length === 0 ? 0 : 1;
}

main(process.argv.slice(2));
