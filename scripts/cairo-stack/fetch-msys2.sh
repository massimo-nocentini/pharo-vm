#!/bin/sh
# shellcheck disable=SC2018,SC2019,SC2013,SC2086  # DLL names are ASCII; word splitting of lists is wanted
# The cairo stack of the Windows VMs: MSYS2's binary packages, pinned by URL
# and SHA256 in the msys2 section of cmake/cairo-stack/pins.txt (decision U2,
# docs/CairoStack.md).
#
#   fetch-msys2.sh --prefix <dir> [--env ucrt64|clangarm64 | --arch x86_64|arm64]
#                  [--features <list>] [--deps <dir>] [--downloads <dir>] [--work <dir>]
#                  [--pins <file>] [--sources <dir> [--no-runtime-sources]]
#       Finds every pinned package of the environment that the features need
#       (in --deps, then in $PHARO_CAIRO_STACK_DOWNLOADS, then in --downloads,
#       else downloads it from its URL or the msys2 mirror), checks every
#       SHA256 before it extracts anything, and puts into <prefix>:
#           bin/*.dll                      the DLLs of the pinned packages
#           bin/glib-mkenums-script.py     glib2's glib-mkenums (the Python script;
#                                          bin/glib-mkenums.exe only launches it)
#           include/                       their headers (cairo.h, cairo-ft.h, ft2build.h...)
#           lib/*.dll.a                    their import libraries
#           lib/<name>/include/            (glib2's lib/glib-2.0/include/glibconfig.h)
#           lib/pkgconfig/*.pc             their pkg-config files, every /<env> path
#                                          rewritten to <prefix> ("cygpath -m" form
#                                          in MSYS2: native programs read them)
#           share/pharo-cairo-stack/
#               manifest.txt               (docs/CairoStack.md, "The manifest")
#               CAIRO-STACK.txt            the index of the stack, for the bundle
#               dlls.txt                   the DLLs the bundle needs:
#                                          "<dll> <kind> <package> <sha256> <features>"
#               licenses/<package>/...     share/licenses of each package
#               msys2/<package>.PKGINFO    the packages' own metadata
#       --features is a comma list of pango and poppler (default pango,poppler;
#       "none" or empty: none; build.sh's --only freetype means none): the
#       packages fetched are those owning a DLL whose msys2-dll line names core
#       or an enabled feature (its 5th field).  With --sources <dir>, it also
#       fetches and checks the source packages of those packages (the LGPL
#       source assets of a release, U10) into <dir>; with --no-runtime-sources,
#       not those of the runtime packages (gcc, winpthreads, libc++: about 350
#       MB of the 450).
#
#   fetch-msys2.sh --closure --env <env> [--work <dir>] [--sources <dir>] [--pins <file>]
#       The bump: reads the live repository (https://repo.msys2.org/mingw/<env>/
#       <env>.files), walks the DLL imports from the roots of each "msys2-root
#       <feature> <dll>..." line of pins.txt (core, pango, poppler) through the
#       packages that own them (as ntldd does), and prints the pins.txt lines
#       of the closure: a "msys2" line per package and a "msys2-dll" line per
#       DLL, whose 5th field lists the features whose roots reach it.  With
#       --sources it downloads each source package to hash it (otherwise the
#       source SHA256 is "-", which check-pins.sh refuses).  Needs curl, zstd,
#       tar and objdump (GNU or LLVM).
#
#   fetch-msys2.sh --list --env <env>
#       Prints "<sha256> <file> <url>" for every pinned package and source
#       package of the environment (for mirroring them, docs/CairoStack.md).
#
# POSIX sh: it runs in the MSYS2 shell of the Windows jobs, and here.

set -eu

script_version=2
here=$(cd "$(dirname "$0")" && pwd)
pins=$here/../../cmake/cairo-stack/pins.txt
mode=fetch
env=
arch=
prefix=
deps=
downloads=
work=
sources=
runtime_sources=yes
features=pango,poppler
features_given=no
only=

die() { echo "fetch-msys2.sh: $*" >&2; exit 1; }
note() { echo "fetch-msys2.sh: $*" >&2; }
usage() { sed -n '2,/^# POSIX sh/p' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }

while [ $# -gt 0 ]; do
    case $1 in
        --prefix) prefix=$2; shift 2 ;;
        --env) env=$2; shift 2 ;;
        --arch) arch=$2; shift 2 ;;
        --deps) deps=$2; shift 2 ;;
        --downloads) downloads=$2; shift 2 ;;
        --work) work=$2; shift 2 ;;
        --pins) pins=$2; shift 2 ;;
        --sources) sources=$2; shift 2 ;;
        --no-runtime-sources) runtime_sources=no; shift ;;
        --closure) mode=closure; shift ;;
        --list) mode=list; shift ;;
        --platform) [ "$2" = mingw ] || die "--platform $2: only mingw"; shift 2 ;;
        --features) features=$2; features_given=yes; shift 2 ;;
        --only) only=$2; shift 2 ;;   # build.sh's --only freetype: core only (the same closure)
        -h|--help) usage 0 ;;
        *) echo "fetch-msys2.sh: unknown argument $1" >&2; usage 1 >&2 ;;
    esac
done

if [ -z "$env" ]; then
    case $arch in
        x86_64|AMD64|amd64) env=ucrt64 ;;
        arm64|ARM64|aarch64) env=clangarm64 ;;
        '') case ${MSYSTEM:-} in
                UCRT64) env=ucrt64 ;;
                CLANGARM64) env=clangarm64 ;;
                *) die "no --env, no --arch and MSYSTEM is '${MSYSTEM:-}': say --env ucrt64 or --env clangarm64" ;;
            esac ;;
        *) die "--arch $arch: x86_64 or arm64" ;;
    esac
fi
case $env in
    ucrt64) pkgprefix=mingw-w64-ucrt-x86_64; arch=x86_64 ;;
    clangarm64) pkgprefix=mingw-w64-clang-aarch64; arch=arm64 ;;
    *) die "--env $env: ucrt64 or clangarm64" ;;
esac
repo=https://repo.msys2.org/mingw

# The features (docs/CairoStack.md): "core" (cairo, FreeType, libffi-8.dll and
# the C/C++ runtime) always, pango and poppler on demand; feature_list is the
# enabled ones in the fixed order "pango poppler".
if [ "$features_given" = no ] && [ "$only" = freetype ]; then features=; fi
for f in $(echo "$features" | tr ',' ' '); do
    case $f in pango|poppler|none) ;; *) die "--features $features: a comma list of pango and poppler (or none)" ;; esac
done
feature_list=
for f in pango poppler; do
    case ",$features," in *",$f,"*) feature_list="$feature_list $f" ;; esac
done
feature_list=${feature_list# }

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
    else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

# download <url> <file>: HTTPS only, into <file>.part first
download() {
    rm -f "$2.part"
    if curl -fsSL --proto '=https' --tlsv1.2 --retry 3 --connect-timeout 30 -o "$2.part" "$1"; then
        mv "$2.part" "$2"
    else
        rm -f "$2.part"
        return 1
    fi
}

# fetch <file> <sha256> <url> [<mirror url>]: prints the path of a checked copy
fetch() {
    f_file=$1 f_sha=$2 f_url=$3 f_mirror=${4:-}
    for f_dir in "$deps" "${PHARO_CAIRO_STACK_DOWNLOADS:-}" "$downloads"; do
        [ -n "$f_dir" ] && [ -f "$f_dir/$f_file" ] || continue
        f_got=$(sha256 "$f_dir/$f_file")
        [ "$f_got" = "$f_sha" ] || die "$f_dir/$f_file has SHA256 $f_got, pins.txt says $f_sha: nothing extracted"
        echo "$f_dir/$f_file"
        return 0
    done
    [ -n "$downloads" ] || die "$f_file is in no --deps or \$PHARO_CAIRO_STACK_DOWNLOADS directory, and there is no --downloads directory"
    mkdir -p "$downloads"
    for f_from in "$f_url" $f_mirror; do
        note "downloading $f_from"
        if download "$f_from" "$downloads/$f_file"; then
            f_got=$(sha256 "$downloads/$f_file")
            if [ "$f_got" = "$f_sha" ]; then
                echo "$downloads/$f_file"
                return 0
            fi
            rm -f "$downloads/$f_file"
            die "$f_from has SHA256 $f_got, pins.txt says $f_sha: nothing extracted"
        fi
    done
    die "cannot download $f_file (from $f_url${f_mirror:+ or $f_mirror})"
}

# The pinned lines of the environment: "<kind> <name> <version> <sha> <url> <srcsha> <srcurl>"
pinned_all() {
    awk -v env="$env" '$1 == "msys2" && $2 == env { print $3, $4, $5, $6, $7, $8, $9 }' "$pins"
}
mirror_base=$(awk '$1 == "msys2-mirror" { print $2; exit }' "$pins" 2>/dev/null || true)
mirror_of() { [ -n "$mirror_base" ] && echo "$mirror_base/$1" || true; }

# objdump that reads PE files of this architecture: "<tool>"
find_objdump() {
    for t in ${OBJDUMP:-} llvm-objdump llvm-objdump-21 llvm-objdump-20 llvm-objdump-19 llvm-objdump-18 objdump; do
        command -v "$t" >/dev/null 2>&1 || continue
        if "$t" -p "$1" 2>/dev/null | grep -q 'DLL Name:'; then echo "$t"; return 0; fi
    done
    return 1
}
imports() { "$objdump" -p "$1" | sed -n 's/^[[:space:]]*DLL Name:[[:space:]]*//p' | tr -d '\r'; }

# The DLLs of Windows itself (and of its universal C runtime): an import of one
# of these is not part of the closure.  check.sh has the same list.
is_system_dll() {
    case $(echo "$1" | tr 'A-Z' 'a-z') in
        api-ms-win-*|ext-ms-win-*|kernel32.dll|kernelbase.dll|user32.dll|gdi32.dll|msimg32.dll|ole32.dll|oleaut32.dll|\
        advapi32.dll|shell32.dll|shlwapi.dll|ws2_32.dll|dnsapi.dll|iphlpapi.dll|userenv.dll|bcrypt.dll|crypt32.dll|\
        secur32.dll|ncrypt.dll|dwrite.dll|d2d1.dll|d3d11.dll|dxgi.dll|windowscodecs.dll|usp10.dll|comdlg32.dll|\
        comctl32.dll|winmm.dll|version.dll|setupapi.dll|rpcrt4.dll|ntdll.dll|psapi.dll|dbghelp.dll|msvcrt.dll|\
        ucrtbase.dll|imm32.dll|uxtheme.dll|dwmapi.dll|winspool.drv|mpr.dll|netapi32.dll|wtsapi32.dll|\
        bcryptprimitives.dll|cfgmgr32.dll|propsys.dll|shcore.dll|mscms.dll|opengl32.dll|hid.dll) return 0 ;;
    esac
    return 1
}

kind_of_dll() {
    case $(echo "$1" | tr 'A-Z' 'a-z') in
        libgcc_s_seh-1.dll|libstdc++-6.dll|libwinpthread-1.dll|libc++.dll|libunwind.dll|libatomic-1.dll|libssp-0.dll) echo runtime ;;
        libcairo-2.dll|libfreetype-6.dll) echo root ;;
        *) echo lib ;;
    esac
}

# ---------------------------------------------------------------- --closure
if [ "$mode" = closure ]; then
    [ -n "$work" ] || work=${TMPDIR:-/tmp}/pharo-cairo-msys2-$env.$$
    mkdir -p "$work/pkgs" "$work/x" "$work/db"
    downloads=$work/pkgs
    for t in curl zstd tar awk; do command -v $t >/dev/null 2>&1 || die "--closure needs $t"; done
    note "reading $repo/$env/$env.files"
    download "$repo/$env/$env.files" "$work/$env.files" || die "cannot download $repo/$env/$env.files"
    rm -rf "$work/db"; mkdir -p "$work/db"
    zstd -dc "$work/$env.files" | tar -xf - -C "$work/db"
    # One record per package: "P <name> <file> <version> <base> <sha256>", and
    # "F <name> <dll lower-case> <dll>" for each DLL of its bin/
    for d in "$work/db"/*/; do
        printf '@@PKG@@\n'
        cat "$d/desc"
        [ -f "$d/files" ] && cat "$d/files"
        printf '\n'
    done | tr -d '\r' | awk -v env="$env" '
        function flush() { if (name != "") print "P", name, file, version, base, sha }
        /^@@PKG@@$/ { flush(); name = file = version = base = sha = ""; key = ""; next }
        /^%[A-Z0-9]+%$/ { key = $0; next }
        $0 == "" { key = ""; next }
        key == "%NAME%" { name = $0 }
        key == "%FILENAME%" { file = $0 }
        key == "%VERSION%" { version = $0 }
        key == "%BASE%" { base = $0 }
        key == "%SHA256SUM%" { sha = $0 }
        key == "%FILES%" && $0 ~ ("^" env "/bin/[^/]+\\.dll$") {
            dll = $0; sub(".*/", "", dll); print "F", name, tolower(dll), dll
        }
        END { flush() }' > "$work/index.txt"
    [ -s "$work/index.txt" ] || die "no package in $env.files"

    pkg_of_dll() { awk -v d="$(echo "$1" | tr 'A-Z' 'a-z')" '$1 == "F" && $3 == d { print $2; exit }' "$work/index.txt"; }
    pkg_field() { awk -v n="$1" -v i="$2" '$1 == "P" && $2 == n { print $i; exit }' "$work/index.txt"; }

    # get <package>: downloaded, checked against the database, extracted into $work/x/<package>
    get() {
        [ -d "$work/x/$1" ] && return 0
        g_file=$(pkg_field "$1" 3); g_sha=$(pkg_field "$1" 6)
        [ -n "$g_file" ] || die "no package $1 in $env.files"
        g_path=$(fetch "$g_file" "$g_sha" "$repo/$env/$g_file")
        mkdir -p "$work/x/$1"
        zstd -dc "$g_path" | tar -xf - -C "$work/x/$1"
    }

    # The roots: "msys2-root <feature> <dll>..." lines of pins.txt
    [ -f "$pins" ] || die "no $pins"
    bad=$(awk '$1 == "msys2-root" && (NF < 3 || ($2 != "core" && $2 != "pango" && $2 != "poppler"))' "$pins")
    [ -z "$bad" ] || die "$pins: bad msys2-root line (msys2-root core|pango|poppler <dll>...): $bad"
    roots_of() { awk -v f="$1" '$1 == "msys2-root" && $2 == f { for (i = 3; i <= NF; i++) printf "%s ", $i }' "$pins"; }
    [ -n "$(roots_of core)" ] || die "$pins has no msys2-root core line"

    : > "$work/dlls.txt"        # "<dll> <package>", in the order found
    : > "$work/features.txt"    # "<dll lower-case> <feature>": the feature's roots reach it
    : > "$work/system.txt"
    rm -rf "$work/imports"; mkdir -p "$work/imports"    # <dll lower-case>: its non-system imports
    objdump=
    lc() { echo "$1" | tr 'A-Z' 'a-z'; }
    # walk <feature> <roots>: one walk per feature, each DLL read once
    walk() {
        w_feature=$1 w_queue=$2
        : > "$work/seen.txt"
        while :; do
            set -- $w_queue
            [ $# -gt 0 ] || break
            dll=$1; shift; w_queue=$*
            l=$(lc "$dll")
            if grep -qxF "$l" "$work/seen.txt"; then continue; fi
            echo "$l" >> "$work/seen.txt"
            echo "$l $w_feature" >> "$work/features.txt"
            if [ ! -f "$work/imports/$l" ]; then
                pkg=$(pkg_of_dll "$dll")
                [ -n "$pkg" ] || die "no package of $env has bin/$dll"
                get "$pkg"
                path=$(find "$work/x/$pkg/$env/bin" -iname "$dll" | head -n 1)
                [ -n "$path" ] || die "$pkg has no $env/bin/$dll"
                echo "$(basename "$path") $pkg" >> "$work/dlls.txt"
                [ -n "$objdump" ] || objdump=$(find_objdump "$path") || die "no objdump reads $path (set OBJDUMP)"
                : > "$work/imports/$l"
                for imp in $(imports "$path"); do
                    if is_system_dll "$imp"; then
                        echo "$imp" >> "$work/system.txt"
                    elif [ -n "$(pkg_of_dll "$imp")" ]; then
                        echo "$imp" >> "$work/imports/$l"
                    else
                        die "$dll imports $imp, which is neither a system DLL nor in a package of $env"
                    fi
                done
            fi
            w_queue="$w_queue $(tr '\n' ' ' < "$work/imports/$l")"
        done
    }
    for feature in core pango poppler; do
        r=$(roots_of $feature)
        [ -z "$r" ] || walk "$feature" "$r"
    done
    # feats_of <dll>: the features whose roots reach it, "core,pango,poppler" order
    feats_of() {
        awk -v d="$(lc "$1")" '$1 == d { f[$2] = 1 }
            END { s = ""; n = split("core pango poppler", o, " ")
                  for (i = 1; i <= n; i++) if (o[i] in f) s = s (s == "" ? "" : ",") o[i]
                  print s }' "$work/features.txt"
    }

    # pactree's view, for the record: the packages the dependencies of the
    # roots' packages reach that own no DLL of the closure
    for d in "$work/db"/*/; do
        printf '@@PKG@@\n'; cat "$d/desc"; printf '\n'
    done | tr -d '\r' | awk '
        /^@@PKG@@$/ { name = ""; key = ""; next }
        /^%[A-Z0-9]+%$/ { key = $0; next }
        $0 == "" { key = ""; next }
        key == "%NAME%" { name = $0; print "N", name, name }
        key == "%DEPENDS%" { d = $0; sub(/[<>=].*/, "", d); print "D", name, d }
        key == "%PROVIDES%" { p = $0; sub(/[<>=].*/, "", p); print "N", p, name }' > "$work/deps.txt"
    rootpkgs=
    for r in $(awk '$1 == "msys2-root" { for (i = 3; i <= NF; i++) print $i }' "$pins" | sort -u); do
        rootpkgs="$rootpkgs $(pkg_of_dll "$r")"
    done
    awk -v roots="${rootpkgs# }" '
        $1 == "N" { if (!($2 in prov)) prov[$2] = $3; else if ($2 == $3) prov[$2] = $3 }
        $1 == "D" { deps[$2] = deps[$2] " " $3 }
        END {
            n = split(roots, q, " ")
            for (i = 1; i <= n; i++) seen[q[i]] = 1
            while (n > 0) {
                p = q[n--]; m = split(deps[p], ds, " ")
                for (j = 1; j <= m; j++) {
                    r = (ds[j] in prov) ? prov[ds[j]] : ds[j]
                    if (!(r in seen)) { seen[r] = 1; q[++n] = r }
                }
            }
            for (p in seen) print p
        }' "$work/deps.txt" | sort > "$work/pactree.txt"

    echo "# msys2 $env: the DLL closure of the msys2-root lines (core, pango, poppler), computed $(date -u +%Y-%m-%d)"
    echo "# from $repo/$env/$env.files by scripts/cairo-stack/fetch-msys2.sh --closure --env $env."
    echo "# msys2 <env> <kind> <package> <version> <sha256> <url> <source sha256> <source url>"
    for pkg in $(awk '{ print $2 }' "$work/dlls.txt" | awk '!seen[$0]++'); do
        version=$(pkg_field "$pkg" 4); file=$(pkg_field "$pkg" 3); sha=$(pkg_field "$pkg" 6); base=$(pkg_field "$pkg" 5)
        kind=lib
        for dll in $(awk -v p="$pkg" '$2 == p { print $1 }' "$work/dlls.txt"); do
            k=$(kind_of_dll "$dll"); [ "$k" = lib ] || kind=$k
        done
        srcfile=$base-$version.src.tar.zst
        srcsha=-
        if [ -n "$sources" ]; then
            mkdir -p "$sources"
            if [ ! -f "$sources/$srcfile" ]; then
                note "downloading $repo/sources/$srcfile"
                download "$repo/sources/$srcfile" "$sources/$srcfile" || die "no source package $repo/sources/$srcfile"
            fi
            srcsha=$(sha256 "$sources/$srcfile")
        fi
        echo "msys2 $env $kind $pkg $version $sha $repo/$env/$file $srcsha $repo/sources/$srcfile"
    done
    echo "# msys2-dll <env> <dll> <package> <features>: the DLLs of the closure (check.sh's import allow-list, the bundle's DLLs)"
    while read -r dll pkg; do
        echo "msys2-dll $env $dll $pkg $(feats_of "$dll")"
    done < "$work/dlls.txt"
    # (comment lines wrapped at 136 columns, continued with "#     ")
    wrap() {
        awk '{ l = ""
               for (i = 1; i <= NF; i++)
                   if (l != "" && length(l) + 1 + length($i) > 136) { print l; l = "#     " $i }
                   else l = (l == "") ? $i : l " " $i
               if (l != "") print l }'
    }
    printf '# system DLLs imported: %s\n' "$(sort -fu "$work/system.txt" | tr '\n' ' ')" | wrap
    printf '# not pinned (reached by the dependencies, but own no DLL of the closure): %s\n' \
        "$(awk 'NR == FNR { p[$2] = 1; next } !($1 in p)' "$work/dlls.txt" "$work/pactree.txt" | sed "s/^$pkgprefix-//" | tr '\n' ' ')" | wrap
    exit 0
fi

# ---------------------------------------------------------------- --list
[ -f "$pins" ] || die "no $pins"
[ -n "$(pinned_all)" ] || die "$pins has no msys2 line for $env"
if [ "$mode" = list ]; then
    pinned_all | while read -r kind name version sha url srcsha srcurl; do
        echo "$sha ${url##*/} $url"
        echo "$srcsha ${srcurl##*/} $srcurl"
    done
    exit 0
fi

# ---------------------------------------------------------------- fetch
[ -n "$prefix" ] || die "no --prefix"
for t in zstd tar; do command -v $t >/dev/null 2>&1 || die "needs $t"; done
case $prefix in /*|[A-Za-z]:*) ;; *) prefix=$(pwd)/$prefix ;; esac
# (a prefix that holds only empty directories counts as fresh: Ninja makes
# the directories of an ExternalProject's byproducts before running it)
if [ -d "$prefix" ] && [ -n "$(find "$prefix" ! -type d 2>/dev/null | head -n 1)" ] && [ ! -f "$prefix/.pharo-cairo-stack" ]; then
    die "$prefix is not empty and is not a cairo stack prefix (no .pharo-cairo-stack): not touching it"
fi
[ -n "$work" ] || work=$prefix.work
[ -n "$downloads" ] || downloads=$work/downloads

# 1. every file found and checked before anything is extracted: the packages
# that own a DLL of core or of an enabled feature ("<dll> <package> <features>")
mkdir -p "$work"
awk -v env="$env" -v want="core $feature_list" '
    BEGIN { n = split(want, w, " "); for (i = 1; i <= n; i++) on[w[i]] = 1 }
    $1 == "msys2-dll" && $2 == env {
        f = (NF >= 5) ? $5 : "core"; m = split(f, fs, ","); s = 0
        for (i = 1; i <= m; i++) if (fs[i] in on) s = 1
        if (s) print $3, $4, f
    }' "$pins" > "$work/dlls.txt"
[ -s "$work/dlls.txt" ] || die "$pins has no msys2-dll line for $env"
pinned_all > "$work/pinned-all.txt"
awk 'NR == FNR { p[$2] = 1; next } ($2 in p)' "$work/dlls.txt" "$work/pinned-all.txt" > "$work/pinned.txt"
: > "$work/files.txt"
while read -r kind name version sha url srcsha srcurl; do
    path=$(fetch "${url##*/}" "$sha" "$url" "$(mirror_of "${url##*/}")")
    echo "$name $path" >> "$work/files.txt"
    if [ -n "$sources" ] && { [ "$kind" != runtime ] || [ "$runtime_sources" = yes ]; }; then
        [ "$srcsha" != - ] || die "pins.txt has no source SHA256 for $name"
        src=$(downloads=$sources deps='' PHARO_CAIRO_STACK_DOWNLOADS='' fetch "${srcurl##*/}" "$srcsha" "$srcurl" "$(mirror_of "${srcurl##*/}")")
        [ -f "$sources/${srcurl##*/}" ] || cp "$src" "$sources/"
    fi
done < "$work/pinned.txt"

# 2. extraction, into a fresh prefix
rm -rf "$prefix" "$work/x"
mkdir -p "$prefix/bin" "$prefix/include" "$prefix/lib/pkgconfig" "$prefix/share/pharo-cairo-stack/licenses" "$prefix/share/pharo-cairo-stack/msys2" "$work/x"
: > "$prefix/.pharo-cairo-stack"
# the prefix as the .pc files name it: native Windows programs (pkgconf, cmake)
# read them, so C:/msys64/... in MSYS2 ("cygpath -m"); the plain path elsewhere
if command -v cygpath >/dev/null 2>&1; then pc_prefix=$(cygpath -m "$prefix"); else pc_prefix=$prefix; fi
pc_prefix_sed=$(printf '%s\n' "$pc_prefix" | sed 's/[#&\\]/\\&/g')
while read -r name path; do
    mkdir -p "$work/x/$name"
    zstd -dc "$path" | tar -xf - -C "$work/x/$name"
    root=$work/x/$name/$env
    [ -d "$root" ] || die "$path has no $env/ directory"
    if [ -d "$root/bin" ]; then
        find "$root/bin" -maxdepth 1 -name '*.dll' -exec cp -p {} "$prefix/bin/" \;
    fi
    [ -d "$root/include" ] && cp -R "$root/include/." "$prefix/include/"
    if [ -d "$root/lib" ]; then
        find "$root/lib" -maxdepth 1 -name '*.dll.a' -exec cp -p {} "$prefix/lib/" \;
        for inc in "$root"/lib/*/include; do
            [ -d "$inc" ] || continue
            mkdir -p "$prefix/lib/${inc#"$root/lib/"}"
            cp -R "$inc/." "$prefix/lib/${inc#"$root/lib/"}/"
        done
        for pc in "$root"/lib/pkgconfig/*.pc; do      # (some have CRLF line ends)
            [ -f "$pc" ] || continue
            tr -d '\r' < "$pc" | sed -e "s#=/$env/#=$pc_prefix_sed/#g" -e "s#=/$env\$#=$pc_prefix_sed#" \
                -e "s#-L/$env/#-L$pc_prefix_sed/#g" -e "s#-I/$env/#-I$pc_prefix_sed/#g" > "$prefix/lib/pkgconfig/${pc##*/}"
        done
    fi
    if [ -f "$root/bin/glib-mkenums-script.py" ]; then
        cp -p "$root/bin/glib-mkenums-script.py" "$prefix/bin/"
    fi
    if [ -d "$root/share/licenses" ]; then
        for l in "$root/share/licenses"/*; do
            [ -d "$l" ] || continue
            mkdir -p "$prefix/share/pharo-cairo-stack/licenses/${name#"$pkgprefix-"}"
            cp -R "$l/." "$prefix/share/pharo-cairo-stack/licenses/${name#"$pkgprefix-"}/"
        done
    fi
    [ -f "$work/x/$name/.PKGINFO" ] && cp "$work/x/$name/.PKGINFO" "$prefix/share/pharo-cairo-stack/msys2/$name.PKGINFO"
done < "$work/files.txt"

# 3. the DLLs of the closure that the features need, each present
: > "$prefix/share/pharo-cairo-stack/dlls.txt"
while read -r dll pkg feats; do
    [ -f "$prefix/bin/$dll" ] || die "$dll (of $pkg) is not in $prefix/bin"
    kind=$(awk -v n="$pkg" '$2 == n { print $1; exit }' "$work/pinned.txt")
    echo "$dll $kind $pkg $(sha256 "$prefix/bin/$dll") $feats" >> "$prefix/share/pharo-cairo-stack/dlls.txt"
done < "$work/dlls.txt"
for h in cairo/cairo.h cairo/cairo-ft.h freetype2/ft2build.h freetype2/freetype/freetype.h fontconfig/fontconfig.h; do
    [ -f "$prefix/include/$h" ] || die "no include/$h in the packages"
done
if [ -n "$feature_list" ]; then
    [ -f "$prefix/lib/glib-2.0/include/glibconfig.h" ] || die "no lib/glib-2.0/include/glibconfig.h in the packages"
fi
case " $feature_list " in
    *" poppler "*) [ -f "$prefix/bin/glib-mkenums-script.py" ] || die "no bin/glib-mkenums-script.py in the packages (poppler's build runs it)" ;;
esac
left=$(grep -lE "(=|-L|-I)/$env(/|\$)" "$prefix"/lib/pkgconfig/*.pc 2>/dev/null || true)
[ -z "$left" ] || die "a .pc file still names /$env: $left"

# 4. the manifest and the index
pkgver() { awk -v n="$pkgprefix-$1" '$2 == n { print $3; exit }' "$work/pinned.txt"; }
cairo_version=$(awk '$1 == "#define" && $2 ~ /^CAIRO_VERSION_(MAJOR|MINOR|MICRO)$/ { v[$2] = $3 }
    END { print v["CAIRO_VERSION_MAJOR"] "." v["CAIRO_VERSION_MINOR"] "." v["CAIRO_VERSION_MICRO"] }' "$prefix/include/cairo/cairo-version.h")
{
    echo "# pharo-cairo-stack manifest (docs/CairoStack.md)"
    echo "format 2"
    echo "pins-sha256 $(sha256 "$pins")"
    echo "platform mingw"
    echo "arch $arch"
    echo "deployment-target -"
    echo "script fetch-msys2.sh $script_version"
    echo "msys2-env $env"
    echo "features ${feature_list:--}"
    echo "cairo-version $cairo_version"
    while read -r kind name version sha url srcsha srcurl; do
        echo "msys2-package $name $version $sha"
    done < "$work/pinned.txt"
} > "$prefix/share/pharo-cairo-stack/manifest.txt"
{
    echo "The cairo stack of this VM (Windows $arch): MSYS2's $env packages, pinned"
    echo "by URL and SHA256 in cmake/cairo-stack/pins.txt of the VM's sources"
    echo "(docs/CairoStack.md).  cairo $(pkgver cairo) and FreeType $(pkgver freetype)"
    echo "are MSYS2's builds, with MSYS2's patches; the source packages (the upstream"
    echo "archives, the patches and the PKGBUILD) are the release's source assets."
    echo "Features besides cairo and FreeType: ${feature_list:-none}."
    echo
    echo "DLLs (name, kind, package, SHA256, features; check.sh compares the bundle's with these):"
    sed 's/^/    dll /' "$prefix/share/pharo-cairo-stack/dlls.txt"
    echo
    echo "Packages (kind, name, version, SHA256, URL, source SHA256, source URL):"
    sed 's/^/    /' "$work/pinned.txt"
} > "$prefix/share/pharo-cairo-stack/CAIRO-STACK.txt"
note "$env: $(wc -l < "$work/pinned.txt" | tr -d ' ') packages, $(wc -l < "$work/dlls.txt" | tr -d ' ') DLLs, cairo $cairo_version, features ${feature_list:-none}, into $prefix"
