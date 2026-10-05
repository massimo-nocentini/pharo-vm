# The symbol table of a library of the FFI registry (Emscripten)
#
#     cmake -DNAME=<c> -DOUTPUT=<ffi_c.c> -DCC=<emcc> -DNM=<llvm-nm>
#           -DCFLAGS=<-m64|-I...> -DWORK=<dir>
#           [-DALIASES=<m|dl>] [-DHEADERS=<stdlib.h|string.h>]
#           [-DSOURCES=<a.c|b.c>] [-DARCHIVES=<lib.a|...>] [-DEXCLUDE=<regex>]
#           [-DSYMBOLS_FILE=<c.txt>] [-DENABLED=<VARIABLE|...>]
#           [-DON_LOAD=<function>]
#           -P genFFILibrary.cmake
#
# cmake/emscripten/ffiRegistry.cmake runs it at build time (the lists have
# '|' between their items).  It writes OUTPUT, a translation unit that
# defines pharoFFILibrary_<NAME> (include/pharovm/emscripten/ffiRegistry.h):
# the symbols of the library, sorted by name (strcmp), with their addresses
# and a NULL signature, its ALIASES and its ON_LOAD function, and prints
# 'FFI library <NAME>: <n> symbols'.
#
# The symbols are the names SYMBOLS_FILE lists outside its exempt sections
# (ffiRegistry.cmake describes the file; ENABLED are the variables of its
# 'unless' sections that are on), the global symbols ARCHIVES define
# (llvm-nm) but those EXCLUDE matches, and the global symbols of the objects
# of SOURCES.  The addresses are taken where the prototypes are the real
# ones: OUTPUT includes HEADERS (the library's public headers) or, for a
# library without headers that declare its functions (the FFI test
# library), its SOURCES themselves, which are then compiled there and not
# linked otherwise.  A probe of every symbol (static void *p = (void *)&s;)
# is compiled first, with CFLAGS.  A symbol of ARCHIVES that is not declared
# there, or whose address is not a constant (a macro such as errno), is left
# out; a name of SYMBOLS_FILE so stops the build, naming it.  So the table
# holds what the headers declare and the library defines, and every name of
# the list.

cmake_minimum_required(VERSION 3.10)

foreach(var NAME OUTPUT CC NM WORK)
    if(NOT ${var})
        message(FATAL_ERROR "genFFILibrary.cmake: ${var} is not set")
    endif()
endforeach()
if(NOT NAME MATCHES "^[A-Za-z_][A-Za-z0-9_]*$")
    message(FATAL_ERROR "genFFILibrary.cmake: NAME '${NAME}' is not a C identifier")
endif()
file(MAKE_DIRECTORY "${WORK}")
foreach(list CFLAGS ALIASES HEADERS SOURCES ARCHIVES ENABLED)
    string(REPLACE "|" ";" ${list} "${${list}}")
endforeach()

# The names of SYMBOLS_FILE: listed (checked) and exempt.  Comments may hold
# ';', which would split a CMake list: they are read as ','.
set(listed "")
set(exempt "")
if(SYMBOLS_FILE)
    file(READ "${SYMBOLS_FILE}" text)
    string(REPLACE ";" "," text "${text}")
    string(REPLACE "\r" "" text "${text}")
    string(REPLACE "\n" ";" lines "${text}")
    set(section "listed")
    set(number 0)
    foreach(line IN LISTS lines)
        math(EXPR number "${number} + 1")
        string(STRIP "${line}" line)
        if(line STREQUAL "")
        elseif(line MATCHES "^#[ \t]*unavailable[ \t]+unless[ \t]+([A-Za-z_][A-Za-z0-9_]*)[ \t]*:")
            if(CMAKE_MATCH_1 IN_LIST ENABLED)
                set(section "listed")
            else()
                set(section "exempt")
            endif()
        elseif(line MATCHES "^#[ \t]*unavailable[ \t]*:")
            set(section "exempt")
        elseif(line MATCHES "^#[ \t]*unavailable")
            message(FATAL_ERROR "${SYMBOLS_FILE}:${number}: a section is '# unavailable: <reason>' "
                "or '# unavailable unless <VARIABLE>: <reason>', not '${line}'")
        elseif(line MATCHES "^#")
        elseif(line MATCHES "^[A-Za-z_][A-Za-z0-9_]*$")
            if(line IN_LIST listed OR line IN_LIST exempt)
                message(FATAL_ERROR "${SYMBOLS_FILE}:${number}: ${line} is listed twice")
            endif()
            list(APPEND ${section} "${line}")
        else()
            message(FATAL_ERROR "${SYMBOLS_FILE}:${number}: '${line}' is not a C identifier")
        endif()
    endforeach()
endif()

# The candidates of the objects: what they define, but EXCLUDE
set(candidates "")
set(objects ${ARCHIVES})
foreach(source IN LISTS SOURCES)
    get_filename_component(base "${source}" NAME_WE)
    set(object "${WORK}/${base}.o")
    execute_process(COMMAND ${CC} ${CFLAGS} -w -c "${source}" -o "${object}"
        RESULT_VARIABLE result ERROR_VARIABLE errors)
    if(NOT result EQUAL 0)
        message(FATAL_ERROR "genFFILibrary.cmake: cannot compile ${source}:\n${errors}")
    endif()
    list(APPEND objects "${object}")
endforeach()
foreach(object IN LISTS objects)
    execute_process(COMMAND ${NM} -g --defined-only -P "${object}"
        OUTPUT_VARIABLE listing RESULT_VARIABLE result ERROR_QUIET)
    if(NOT result EQUAL 0)
        message(FATAL_ERROR "genFFILibrary.cmake: ${NM} cannot read ${object}")
    endif()
    string(REGEX MATCHALL "(^|\n)[A-Za-z_][A-Za-z0-9_]* [TDBRVW] " found "${listing}")
    foreach(line IN LISTS found)
        string(REGEX REPLACE "^\n?([A-Za-z0-9_]+) .*" "\\1" symbol "${line}")
        list(APPEND candidates "${symbol}")
    endforeach()
endforeach()
list(REMOVE_DUPLICATES candidates)
if(EXCLUDE)
    set(kept "")
    foreach(symbol IN LISTS candidates)
        if(NOT symbol MATCHES "${EXCLUDE}")
            list(APPEND kept "${symbol}")
        endif()
    endforeach()
    set(candidates ${kept})
endif()
if(exempt)
    list(REMOVE_ITEM candidates ${exempt})
endif()
list(APPEND candidates ${listed})
list(REMOVE_DUPLICATES candidates)
list(SORT candidates)

set(prologue "/* Generated by cmake/emscripten/genFFILibrary.cmake.  Do not edit. */\n")
foreach(header IN LISTS HEADERS)
    string(APPEND prologue "#include <${header}>\n")
endforeach()
foreach(source IN LISTS SOURCES)
    string(APPEND prologue "#include \"${source}\"\n")
endforeach()
string(APPEND prologue "#include \"pharovm/emscripten/ffiRegistry.h\"\n")
string(REGEX MATCHALL "\n" newlines "${prologue}")
list(LENGTH newlines firstLine)
math(EXPR firstLine "${firstLine} + 1")

# Probe, and leave out the candidates whose lines have errors, until none
# has; a name of SYMBOLS_FILE with an error stops at once
set(probe "${WORK}/probe_${NAME}.c")
set(result 1)
foreach(round RANGE 1 4)
    set(text "${prologue}")
    foreach(symbol IN LISTS candidates)
        string(APPEND text "static void *pharo_probe_${symbol} = (void *)&${symbol};\n")
    endforeach()
    file(WRITE "${probe}" "${text}")
    execute_process(COMMAND ${CC} ${CFLAGS} -fsyntax-only -ferror-limit=0 -w "${probe}"
        RESULT_VARIABLE result ERROR_VARIABLE errors)
    if(result EQUAL 0)
        break()
    endif()
    string(REGEX MATCHALL "probe_${NAME}\\.c:[0-9]+:[0-9]+: error" lines "${errors}")
    if(NOT lines)
        message(FATAL_ERROR "genFFILibrary.cmake: the probe of the headers of ${NAME} fails:\n${errors}")
    endif()
    set(bad "")
    foreach(line IN LISTS lines)
        string(REGEX REPLACE ".*\\.c:([0-9]+):.*" "\\1" number "${line}")
        math(EXPR index "${number} - ${firstLine}")
        list(LENGTH candidates count)
        if(index LESS 0 OR NOT index LESS count)
            message(FATAL_ERROR "genFFILibrary.cmake: the headers of ${NAME} do not compile:\n${errors}")
        endif()
        list(APPEND bad ${index})
    endforeach()
    list(REMOVE_DUPLICATES bad)
    list(SORT bad COMPARE NATURAL)
    set(kept "")
    set(undeclared "")
    set(index 0)
    foreach(symbol IN LISTS candidates)
        list(FIND bad ${index} found)
        if(found EQUAL -1)
            list(APPEND kept "${symbol}")
        elseif(symbol IN_LIST listed)
            list(APPEND undeclared "${symbol}")
        endif()
        math(EXPR index "${index} + 1")
    endforeach()
    if(undeclared)
        string(REPLACE ";" " " undeclared "${undeclared}")
        string(REPLACE ";" " " headers "${HEADERS}${SOURCES}")
        message(FATAL_ERROR "genFFILibrary.cmake: ${SYMBOLS_FILE} lists symbols that the headers "
            "of ${NAME} (${headers}) do not declare, or not as functions or data: ${undeclared}.  "
            "Add the header that declares them, or move them to an '# unavailable' section.\n${errors}")
    endif()
    set(candidates ${kept})
endforeach()
if(NOT result EQUAL 0)
    message(FATAL_ERROR "genFFILibrary.cmake: the probe of ${NAME} still fails:\n${errors}")
endif()

list(LENGTH candidates count)
set(rows "")
foreach(symbol IN LISTS candidates)
    string(APPEND rows "\t{\"${symbol}\", (void *)&${symbol}, NULL},\n")
endforeach()
set(aliases "")
foreach(alias IN LISTS ALIASES)
    string(APPEND aliases "\"${alias}\", ")
endforeach()
set(onLoad "NULL")
set(onLoadDeclaration "")
if(ON_LOAD)
    set(onLoad "${ON_LOAD}")
    set(onLoadDeclaration "\nextern void ${ON_LOAD}(void);\n")
endif()

file(WRITE "${OUTPUT}.tmp" "${prologue}${onLoadDeclaration}
/* ${count} symbols */
static const PharoFFISymbol symbols[] = {
${rows}\t{NULL, NULL, NULL}
};

static const char *const aliases[] = { ${aliases}NULL };

const PharoFFILibrary pharoFFILibrary_${NAME} = {
	\"${NAME}\", aliases, symbols, ${count}, ${onLoad}
};
")
configure_file("${OUTPUT}.tmp" "${OUTPUT}" COPYONLY)
file(REMOVE "${OUTPUT}.tmp")
message(STATUS "FFI library ${NAME}: ${count} symbols")
