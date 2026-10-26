"""Convert the Pharo Slang output for LargeIntegersPlugin into the checked-in
plugins/LargeIntegers/src/common/LargeIntegers.c.

usage: python3 -I convert.py REGEN.c CHECKED-IN.c OUT.c
       python3 -I convert.py --digests REGEN.c CHECKED-IN.c

REGEN.c is what Pharo Slang generates from smalltalksrc/VMMaker/
LargeIntegersPlugin.class.st (regen.st writes it).  CHECKED-IN.c is the
current checked-in file: the conversion takes from it only the text the
generator cannot give (see below), so applying the conversion to the
generator output of an unchanged class reproduces CHECKED-IN.c byte for byte.

The checked-in file keeps the layout of the old Squeak generator
(VMMaker.oscog-eem.2495) it was first made with.  The output is assembled
as follows:

 - Preamble: the header comment and __buildInfo of CHECKED-IN.c, then the
   includes and constants of REGEN.c, then function prototypes made from
   the functions emitted, then the Variables section of CHECKED-IN.c (its
   interpreterProxy imports and module name; REGEN.c lacks the imports the
   conversion of asOop: needs and has a different module name).
 - Functions: those reachable from the exported ones.  The function bodies
   are REGEN.c's, converted (see convert_function), except for the
   functions in VERBATIM, which keep their CHECKED-IN.c text (see PINNED
   below).  Pharo Slang
   also emits standalone copies of the inlined methods; they are dropped
   because nothing calls them.
 - Order: the order of CHECKED-IN.c; a function it does not have yet goes
   before the first function that follows it in REGEN.c.
 - Text between two functions (method comments, and the comments the old
   generator left for inlined methods): that of CHECKED-IN.c, except that a
   method comment that differs from the class's (ignoring white space and
   comment delimiters) is replaced by REGEN.c's.  A new function gets
   REGEN.c's comment.
 - Statement comments: Pharo Slang drops the comments inside a method body
   that the old generator emitted.  A comment block of CHECKED-IN.c that
   precedes a statement line is kept when that line is still generated, once.
 - Trailer (export table, accessor depths): REGEN.c's.

Messages go to stderr.  Exit status: 0 on success, 1 when a pinned part
(see VERBATIM and VARIABLES) no longer matches the class or was edited by
hand, 2 on a usage error or an input this script does not understand
(including any internal error).
"""
import hashlib
import re
import sys

# PINNED parts: C that the output takes from CHECKED-IN.c instead of
# deriving it from the class.  It is equivalent to the class, but nothing
# derives it from it, so each part is pinned by two digests (SHA-256, first
# 16 hex digits): (1) of Pharo Slang's output for it, converted, and (2) of
# its text in CHECKED-IN.c.  When (1) changes, an edit of the class changed
# that C; when (2) changes, the checked-in C was edited by hand.  Either
# stops the conversion (exit 1) with the name of the part: make the
# checked-in C match the class by hand, then refresh the digests (--digests
# prints both tables, ready to paste here).
#
# VERBATIM: functions that keep the text of the old generator, which
# differs from the conversion of Pharo Slang's output in ways no rule here
# covers (the old generator's casts, parentheses and ?: layout, a dead
# store, typed locals, the guarded proxy imports of setInterpreter, ...).
# Removing a function from this table takes the generated text instead.
#
# VARIABLES: the Variables section, from '/*** Variables ***/' to the first
# function: the class's constants (declareCVarsIn:), its variables, the
# module name and version, the interpreterProxy imports.  The checked-in
# one differs from Pharo Slang's (the module name, the imports the
# conversion of asOop: needs, the guards of the old generator).
VERBATIM = {
    'cDigitHighBitlen': ('2dcca7b8316dbc34', '4c1835b8e3e5f025'),
    'cDigitOfat': ('94f87306bb6acd03', '4111e44481a84bed'),
    'cHighBit32': ('c5b455d951e101fd', 'e63fc8ef00eedf6a'),
    'largeIntgrowTo': ('0684712300701279', 'b4c0dcb8f2b8fa32'),
    'normalizeNegative': ('7f65225ce27aa64f', '9cc9997219ced654'),
    'normalizePositive': ('44f47a70b5188b89', '083ed569dd43cf70'),
    'primDigitCompare': ('9bd45effab9d2bfa', '992c6f09f8143e4e'),
    'setInterpreter': ('d7a9845301207d96', '7fd7e24caed0328f'),
}
VARIABLES = ('74165eeb2ccc2a35', '80d18bc2cfff47a8')


def die(msg, status=2):
    sys.stderr.write('convert.py: %s\n' % msg)
    sys.exit(status)


# --- parsing -----------------------------------------------------------------

def comment_start(lines, i):
    """lines[i] ends a /* */ comment: answer the index of its first line."""
    while not lines[i].lstrip().startswith('/*'):
        i -= 1
        if i < 0:
            die('unbalanced comment')
    return i


def parse(lines):
    """Answer (functions, order).  functions maps a C function name to
    {'gap': (g, m), 'body': (m, e)}: lines[m] is the generator's marker
    comment ('/* Class>>#selector */', indented by a tab in the old layout),
    lines[m+1] the return type, lines[m+2] 'name(args)', lines[e-1] the
    closing '}'; lines[g:m] are the blank lines and comments in front of it."""
    funcs, order = {}, []
    for i, line in enumerate(lines):
        m = re.match(r'^(\w+)\(.*\)$', line)
        if not m or i + 1 >= len(lines) or lines[i + 1] != '{':
            continue
        name, marker = m.group(1), i - 2
        if not re.match(r'^\t?/\* \w+>>#\S+ \*/$', lines[marker]):
            die('no generator marker above %s' % name)
        end = i + 1
        while lines[end] != '}':
            end += 1
        g = marker
        while g > 0:
            prev = lines[g - 1]
            if prev.strip() == '':
                g -= 1
            elif prev.rstrip().endswith('*/') and not prev.startswith('#'):
                g = comment_start(lines, g - 1)
            else:
                break
        funcs[name] = {'gap': (g, marker), 'body': (marker, end + 1)}
        order.append(name)
    if not order:
        die('no functions found')
    return funcs, order


# --- conversion of Pharo Slang's text to the checked-in conventions ----------

def close_paren(text, i):
    """Index just past the ')' that closes the '(' at text[i]."""
    depth = 0
    while i < len(text):
        if text[i] == '(':
            depth += 1
        elif text[i] == ')':
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    die('unbalanced parentheses')


def translate_asOop(text):
    """Pharo Slang leaves 'x asOop: SmallInteger' / 'asOop: Boolean' (the
    SmartSyntax return conversions) as 'asOop(x, _SmallInteger)'; translate
    them as the old generator did."""
    while True:
        i = text.find('asOop(')
        if i < 0:
            return text
        j = close_paren(text, i + 5)
        inner = text[i + 6:j - 1]
        k = inner.rfind(',')
        expr, kind = inner[:k].strip(), inner[k + 1:].strip()
        if kind == '_SmallInteger':
            new = 'integerObjectOf(%s)' % expr
        elif kind == '_Boolean':
            new = '((%s) ? trueObject() : falseObject())' % expr
        else:
            die('asOop: of an unknown class: %s' % kind)
        text = text[:i] + new + text[j:]


def convert_text(lines):
    t = '\n'.join(lines)
    t = translate_asOop(t)
    # inlined expression sends: '(/* begin x */ e /* end x */)' -> '(e)'
    t = re.sub(r'\(/\* begin \w+:? \*/ (.*?) /\* end \w+:? \*/\)', r'(\1)', t)
    # the old generator has no '/* end ... */' markers
    t = re.sub(r'(?m)^[ \t]*/\* end [^*]*\*/\n', '', t)
    t = re.sub(r'(?m)^(\t*)debugCode\(.*\);$', r'\1/* missing DebugCode */;', t)
    # Pharo Slang prefixes locals and arguments with '_' ('__return_value'
    # becomes '_return_value', the old generator's name)
    t = re.sub(r'(^|[^A-Za-z0-9_])_([A-Za-z_][A-Za-z0-9_]*)', r'\1\2', t)
    t = re.sub(r'(?m)(\S) +\)', r'\1)', t)
    t = re.sub(r'(?m)^(\t*)\} else \{$', r'\1}\n\1else {', t)
    t = re.sub(r'(?m)^(\t*)\}\n else \{$', r'\1}\n\1else {', t)
    # the block argument of addressOf:put: [:v | ...] (simulation only)
    t = re.sub(r'(?m)^\tsqInt v;\n', '', t)
    t = re.sub(r'(?m)^\tv = 0;\n', '', t)
    # remapOop:in: [...] emits its block as a bare '{ ... }': drop the
    # braces and dedent the block
    out, stack = [], []
    for line in t.split('\n'):
        m = re.match(r'^(\t*)\{$', line)
        if m and out and not re.match(r'^\w', out[-1]):
            stack.append(m.group(1))
            continue
        if stack and line == stack[-1] + '}':
            stack.pop()
            continue
        if stack and line.startswith('\t' * len(stack)):
            line = line[len(stack):]
        out.append(line)
    return out


def is_declaration(line):
    """A local of Pharo Slang's output: '\tsqInt x;', '\tunsigned int *p;',
    '\tunsigned int buf[N];'."""
    return re.match(r'^\t(?!return\b)[A-Za-z_]\w*(?: [A-Za-z_]\w*)*(?: \**)'
                    r'[A-Za-z_]\w*(?:\[\w*\])?;$', line) is not None


def convert_prolog(lines):
    """A primitive: SmartSyntax's prolog is the statements before the
    debugCode line (parameter fetches, the arity check, the argument checks
    of the .st) and the receiver check after it.  Drop the out-parameter
    initialisations ('x = 0;') of the prolog, wrap its '&&' as the old
    generator did, and drop the 'popthenPush(n, null);' that Pharo Slang
    puts before the return of the failed receiver check."""
    if not lines[1].startswith('EXPORT(') or '\t/* missing DebugCode */;' not in lines:
        return lines
    debug = lines.index('\t/* missing DebugCode */;')
    # the declarations, then a blank line; a function without locals has
    # neither
    k = lines.index('{') + 1
    while k < debug and is_declaration(lines[k]):
        k += 1
    decls_end = k if lines[k] == '' else k - 1
    prolog = [l for l in lines[decls_end + 1:debug] if not re.match(r'^\t\w+ = 0;$', l)]
    prolog = '\n'.join(prolog).replace(' && ', '\n\t\t && ').split('\n') if prolog else []
    rest = lines[debug:]
    for k in range(len(rest) - 2):
        if rest[k] == '\tif (failed()) {' and re.match(r'^\t\tpopthenPush\(\d+, null\);$', rest[k + 1]):
            del rest[k + 1]
            break
    return lines[:decls_end + 1] + prolog + rest


def convert_function(regen_lines):
    lines = convert_text(regen_lines)
    lines[0] = '\t' + lines[0]          # the old layout indents the marker
    return convert_prolog(lines)


def statement_comments(old_body, new_body, name):
    """Insert the comment blocks of old_body that precede a statement line
    (with the blank line before them) in front of the same line of new_body,
    when new_body has that line exactly once and not that block yet."""
    new = list(new_body)
    i = 4                               # past the marker, type, name and '{'
    while i < len(old_body):
        line = old_body[i].strip()
        if not line.startswith('/*') or line.startswith('/* begin ') or line == '/* missing DebugCode */;':
            i += 1
            continue
        j = i
        while not old_body[j].rstrip().endswith('*/'):
            j += 1
        start = i - 1 if old_body[i - 1].strip() == '' else i
        block, anchor = old_body[start:j + 1], old_body[j + 1]
        i = j + 1
        if anchor.strip() in ('', '}') or anchor.lstrip().startswith('/*'):
            continue
        hits = [k for k, l in enumerate(new) if l == anchor]
        if len(hits) != 1:
            sys.stderr.write('convert.py: %s: the comment %r of the checked-in C is dropped: '
                             'its statement is not generated once\n' % (name, block[-1].strip()))
        elif new[hits[0] - len(block):hits[0]] != block:
            new[hits[0]:hits[0]] = block
    return new


def own_comment(lines, gap):
    """The comment blocks right in front of a function (the last run of
    non-blank lines of its gap), as (start, end) indexes into lines."""
    g, m = gap
    end = m
    while end > g and lines[end - 1].strip() == '':
        end -= 1
    start = end
    while start > g and lines[start - 1].strip() != '':
        start -= 1
    return start, end


def comment_words(lines):
    return ''.join(''.join(lines).replace('/*', '').replace('*/', '').split())


def digest(lines):
    return hashlib.sha256('\n'.join(lines).encode()).hexdigest()[:16]


# --- assembly -------------------------------------------------------------------

def variables(lines, funcs, order):
    """The Variables section: from '/*** Variables ***/' to the comments
    in front of the first function."""
    try:
        start = lines.index('/*** Variables ***/')
    except ValueError:
        die('section not found: /*** Variables ***/')
    return lines[start:funcs[order[0]]['gap'][0]]


def pinned(REG, rf, rorder, OLD, of, oorder):
    """Map each pinned part to (digest of Pharo Slang's text, digest of the
    checked-in text); None when a function is missing from either."""
    parts = {}
    for name in sorted(VERBATIM):
        s, e = rf[name]['body'] if name in rf else (0, 0)
        os_, oe = of[name]['body'] if name in of else (0, 0)
        parts[name] = (digest(convert_function(REG[s:e])) if name in rf else None,
                       digest(OLD[os_:oe]) if name in of else None)
    parts['(Variables section)'] = (digest(variables(REG, rf, rorder)),
                                    digest(variables(OLD, of, oorder)))
    return parts


def main(argv):
    if len(argv) == 3 and argv[0] == '--digests':
        argv = argv[1:] + [None]
    elif len(argv) != 3 or argv[0].startswith('-'):
        die('usage: convert.py REGEN.c CHECKED-IN.c OUT.c\n'
            '       convert.py --digests REGEN.c CHECKED-IN.c')
    regen_path, old_path, out_path = argv
    REG = open(regen_path).read().split('\n')
    OLD = open(old_path).read().split('\n')
    rf, rorder = parse(REG)
    of, oorder = parse(OLD)
    parts = pinned(REG, rf, rorder, OLD, of, oorder)

    if out_path is None:                # --digests
        print('VERBATIM = {')
        for name in sorted(VERBATIM):
            print("    '%s': ('%s', '%s')," % ((name,) + tuple(d or 'MISSING' for d in parts[name])))
        print('}')
        print("VARIABLES = ('%s', '%s')" % parts['(Variables section)'])
        return 0

    def regen_body(name):
        s, e = rf[name]['body']
        return convert_function(REG[s:e])

    stale = []
    pins = dict(VERBATIM, **{'(Variables section)': VARIABLES})
    for name in sorted(pins):
        (slang, checked), (slang_pin, checked_pin) = parts[name], pins[name]
        if slang is None:
            stale.append('%s: no longer generated' % name)
        elif slang != slang_pin:
            stale.append('%s: the class changed it (digest %s, pinned %s): update the checked-in C '
                         'by hand to match' % (name, slang, slang_pin))
        if checked is None:
            stale.append('%s: not in %s' % (name, old_path))
        elif checked != checked_pin:
            stale.append('%s: its checked-in C was edited (digest %s, pinned %s): undo the edit, '
                         'unless it is the hand update for a change of the class' % (name, checked, checked_pin))
    if stale:
        sys.stderr.write('convert.py: C kept from %s instead of being generated no longer matches '
                         'the pinned digests:\n' % old_path)
        for s in stale:
            sys.stderr.write('  %s\n' % s)
        sys.stderr.write('Once the checked-in C matches the class again, paste the output of --digests\n'
                         'over VERBATIM and VARIABLES in convert.py (to take the generated text of a\n'
                         'function instead, drop it from VERBATIM).\n')
        return 1

    bodies = {}

    def body(name):
        if name not in bodies:
            if name in VERBATIM:
                s, e = of[name]['body']
                bodies[name] = OLD[s:e]
            else:
                bodies[name] = regen_body(name)
                if name in of:
                    s, e = of[name]['body']
                    bodies[name] = statement_comments(OLD[s:e], bodies[name], name)
        return bodies[name]

    def gap(name):
        rs, re_ = own_comment(REG, rf[name]['gap'])
        regen_comment = [l.rstrip() for l in REG[rs:re_]]
        if name not in of:
            return ['', ''] + (regen_comment + [''] if regen_comment else [])
        g, m = of[name]['gap']
        lines = OLD[g:m]
        if not regen_comment:
            return lines
        os_, oe = own_comment(OLD, of[name]['gap'])
        if comment_words(OLD[os_:oe]) == comment_words(regen_comment):
            return lines
        sys.stderr.write('convert.py: %s: method comment taken from the class\n' % name)
        if os_ == oe:
            return lines[:m - g] + regen_comment + ['']
        return OLD[g:os_] + regen_comment + OLD[oe:m]

    # reachability from the exported functions, over the final bodies
    exported = [n for n in rf if REG[rf[n]['body'][0] + 1].startswith('EXPORT(')]
    reach, todo = set(), list(exported)
    while todo:
        name = todo.pop()
        if name in reach:
            continue
        if name not in rf:
            die('%s is called but not generated' % name)
        reach.add(name)
        for ident in re.findall(r'\b(\w+)\(', '\n'.join(body(name))):
            if ident in rf and ident not in reach:
                todo.append(ident)

    order = []
    for name in oorder:
        if name not in reach:
            sys.stderr.write('convert.py: %s is no longer generated: removed with the comments in front of it\n'
                             % name)
            continue
        for new in rorder:
            if new == name:
                break
            if new in reach and new not in of and new not in order:
                order.append(new)
                sys.stderr.write('convert.py: %s is new\n' % new)
        order.append(name)
    for new in rorder:
        if new in reach and new not in order:
            order.append(new)
            sys.stderr.write('convert.py: %s is new\n' % new)

    texts = {name: body(name) for name in order}
    protos = ['%s %s;' % (texts[n][1], texts[n][2]) for n in order]

    try:
        old_includes = OLD.index('#include "config.h"')
        reg_includes = REG.index('#include "config.h"')
        reg_protos = REG.index('/*** Function Prototypes ***/')
        old_protos = OLD.index('/*** Function Prototypes ***/')
    except ValueError as e:
        die('section not found: %s' % e)
    old_vars = OLD.index('', old_protos)        # the blank line after the prototypes
    first = of[oorder[0]]['gap'][0]
    last_end = rf[rorder[-1]]['body'][1]

    out = OLD[:old_includes] + REG[reg_includes:reg_protos + 1] + protos + OLD[old_vars:first]
    for name in order:
        out += gap(name) + texts[name]
    out += REG[last_end:]

    # every interpreterProxy function the functions call must be imported
    imported = set(re.findall(r'^static [^(]*\(\*(\w+)\)\(', '\n'.join(OLD[old_vars:first]), re.M))
    proxy = set(re.findall(r'^static [^(]*\(\*(\w+)\)\(', '\n'.join(REG), re.M)) | {'trueObject', 'falseObject'}
    called = set(re.findall(r'\b(\w+)\(', '\n'.join(sum(texts.values(), []))))
    missing = sorted((called & proxy) - imported)
    if missing:
        die('interpreterProxy functions called but not imported by the Variables section and '
            'setInterpreter of %s: %s; add them there by hand' % (old_path, ', '.join(missing)))

    open(out_path, 'w').write('\n'.join(out))
    return 0


if __name__ == '__main__':
    try:
        status = main(sys.argv[1:])
    except OSError as e:
        die(e)
    except Exception as e:              # never exit 1, which means drift
        die('internal error, on an input it does not understand? %s: %s' % (type(e).__name__, e))
    sys.exit(status)
