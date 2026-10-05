# GNUmakefile - the WebAssembly (Emscripten) build of the Pharo VM
#
# The VM is built with CMake (see README.md).  This file adds the goals of
# its WebAssembly build, which runs CMake out of tree in build-wasm/ (see
# docs/WebAssembly.md):
#
#   make wasm                 build-wasm/node/pharo, the VM for node, and
#                             build-wasm/web/, the site of the browser VM
#   make wasm-check           the node test lanes (tests/wasm/lanes)
#   make wasm-check-browser   the Playwright specs (Playwright from
#                             PLAYWRIGHT_MODULE, or found by node)
#   make wasm-serve           serves build-wasm/web on WASM_PORT (8080)
#   make wasm-clean           removes the wasm build, keeping build-wasm/host,
#                             build-wasm/downloads (the image zip and the
#                             archives of the libraries) and build-wasm/image
#   make wasm-distclean       also removes build-wasm/host, build-wasm/image
#                             and the recorded settings (and with
#                             WASM_CLEAN_DOWNLOADS=1 build-wasm/downloads)
#
# `make wasm' first generates the StackVM sources in build-wasm/host, a
# native CMake tree of this checkout used only for its generate-sources
# target, and whose VMMaker image is refreshed from smalltalksrc when a .st
# file changes, comes or goes.  It then configures build-wasm/cmake with
# emcmake and the initial cache cmake/Emscripten.cache.cmake, and builds it,
# which also stages build-wasm/node and build-wasm/web.
#
# Settings given on the command line (the WASM_* variables below) are
# recorded in build-wasm/config.make, which changes, and so configures the
# wasm build again, only when a setting does.  They are recorded, not
# remembered: a later `make wasm' without them goes back to the defaults.
#
# WASM_BUILDDIR names another build directory than build-wasm, a path with
# no blank and none of the characters make or the shell treats specially
# (such as * ; $ % :).  The goals above use only a new or empty one, one
# that make wasm built in before (it leaves a .make-wasm there) or a
# build-wasm* of the source tree: never the source tree, a directory holding
# it, another source tree, $HOME or /, and never an empty WASM_BUILDDIR.
#
# GNU make reads this file instead of Makefile, so the default goal and
# every goal not defined here go to the Makefile of an in-source CMake build
# when there is one.

SRCDIR := $(patsubst %/,%,$(dir $(abspath $(lastword $(MAKEFILE_LIST)))))

.PHONY: forward-default wasm wasm-check wasm-check-browser wasm-serve wasm-clean wasm-distclean FORCE

# As in the Makefile of CMake, the goals run one after the other (the builds
# they run still share the job slots of -j), so that `make -jN clean all' or
# `make -jN wasm-clean wasm' never run both at once; and there are no
# built-in rules, so that a goal such as src/client.o goes to that Makefile
# too.
.NOTPARALLEL:
.SUFFIXES:

ifneq ($(wildcard Makefile),)
forward-default:
	+@$(MAKE) -f Makefile
.DEFAULT:
	+@$(MAKE) -f Makefile $@
else
forward-default:
	@echo "No in-source CMake build here: see README.md, or run make wasm (docs/WebAssembly.md)" >&2
	@exit 1
endif

GNUmakefile: ;

# Settings

WASM_BUILDDIR ?= build-wasm
WASM_DEBUG ?= 0
WASM_SJLJ ?= wasm
WASM_WEB_MEMORY64 ?= 2
WASM_STACK_SIZE ?= 8MB
WASM_INITIAL_MEMORY ?= 32MB
WASM_MAXIMUM_MEMORY ?= 4GB
WASM_OLD_SPACE_BASE ?= 0x20000000
WASM_SLICE_MS ?= 20
WASM_WORLD ?= ON
WASM_FFI ?= ON
WASM_FREETYPE ?= ON
# cairo: AUTO (built with FreeType), ON or OFF (cmake/emscripten/deps/options.cmake)
WASM_CAIRO ?= AUTO
WASM_CAIRO_PDF ?= OFF
# libgit2, for Iceberg (opt-in), and its smart-HTTP transport over XHR
WASM_LIBGIT2 ?= OFF
WASM_LIBGIT2_HTTP ?= ON
# SDL2, for the image's own OSSDL2Driver in the page sdl.html (opt-in)
WASM_SDL2 ?= OFF
# offline builds: the Pharo 12 image zip, a directory holding the pinned
# archives of the libraries (cmake/emscripten/deps/fetch.cmake), the
# generated sources (a directory holding generated/64), or the VMMaker image
# and the Pharo VM that runs it
WASM_IMAGE_ZIP ?=
WASM_DEPS_DIR ?=
WASM_GENERATED ?=
WASM_VMMAKER_IMAGE ?=
WASM_VMMAKER_VM ?=
# not recorded
WASM_PORT ?= 8080
WASM_JOBS ?= $(shell nproc 2>/dev/null || getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)
WASM_CLEAN_DOWNLOADS ?= 0

# Resolved by the shell: make's own lookup of a plain "node" or "cmake"
# stops at the first one on the PATH even if it is a directory (as in the
# emsdk root and in upstream/emscripten, which emsdk_env.sh puts on the PATH).
# The redirection makes every make hand the command to the shell: before
# 4.3, make runs a plain `command -v cmake' itself, as a program.
ifndef CMAKE
CMAKE := $(firstword $(shell command -v cmake 2>/dev/null) cmake)
endif
ifndef EMCMAKE
EMCMAKE := $(if $(EMSDK),$(EMSDK)/upstream/emscripten/emcmake,$(firstword $(shell command -v emcmake 2>/dev/null) emcmake))
endif
ifndef NODE
NODE := $(firstword $(shell command -v node 2>/dev/null) node)
endif

W := $(abspath $(WASM_BUILDDIR))

# The goals below write into W, and the clean goals remove what they wrote
# there, so W must be a build directory of its own: one path (not empty, as
# an unset variable would make it, so that the recipes never run on /cmake
# and the like), which, with its symbolic links resolved, holds none of the
# characters that make or the shell would not take literally in the recipes
# and the checks, and is neither /, $HOME, the source tree, a directory
# holding it nor any other source tree, and new, empty, one make wasm
# already built in (which it marks with W/.make-wasm; config.make marks one
# of an earlier make wasm), or build-wasm* of the source tree, which
# .gitignore keeps for make wasm (so that the inputs of an offline build can
# be put there first).  Symbolic links are resolved before comparing.
WASM_GOALS := wasm wasm-check wasm-check-browser wasm-serve wasm-clean wasm-distclean
WASM_MARK = echo "make wasm (GNUmakefile of $(SRCDIR)) builds in this directory" >$(W)/.make-wasm
WASM_SPECIAL_CHARACTERS := ' " ` \ $$ ; & | < > ( ) { } [ ] * ? % : \#

# $(call special,PATH): the characters of WASM_SPECIAL_CHARACTERS that PATH
# holds (nothing when it holds none)
special = $(strip $(foreach c,$(WASM_SPECIAL_CHARACTERS),$(findstring $c,$(1))))

# $(call physical,PATH): the absolute PATH with the symbolic links of the
# part of it that exists resolved (nothing when PATH is not absolute, or
# cannot be resolved)
physical = $(shell p='$(1)'; case "$$p" in (/*) ;; (*) exit 1 ;; esac; \
  r=; while test ! -d "$$p"; do r=/$$(basename "$$p")$$r; p=$$(dirname "$$p"); done; \
  p=$$(cd "$$p" && pwd -P) && p=$${p%/}$$r && echo "$${p:-/}")

# The goals make was asked for: on its command line, or .DEFAULT_GOAL
WASM_REQUESTED := $(or $(MAKECMDGOALS),$(.DEFAULT_GOAL))

# The checks of W.  They are made when make reads this file for a goal above
# or a file of W, and otherwise (for a goal that comes from somewhere else)
# by the recipes that write or remove in W, before they do: see
# WASM_CHECK_BUILDDIR.
define WASM_BUILDDIR_CHECKS
ifneq ($(words $(SRCDIR))$(wildcard $(SRCDIR)/GNUmakefile),1$(SRCDIR)/GNUmakefile)
$(error make wasm cannot find its source tree ($(SRCDIR)): the path of the tree must hold no blank)
endif
ifeq ($(strip $(WASM_BUILDDIR)),)
$(error WASM_BUILDDIR is empty: leave it unset for build-wasm, or name a build directory of its own)
endif
ifneq ($(words $(WASM_BUILDDIR)),1)
$(error WASM_BUILDDIR ($(WASM_BUILDDIR)) holds a blank: name a build directory of its own, such as build-wasm)
endif
ifneq ($(words $(W)),1)
$(error WASM_BUILDDIR ($(WASM_BUILDDIR)) is relative to the current directory, $(CURDIR), whose path holds a blank: name an absolute build directory without blanks)
endif
ifneq ($(call special,$(W)),)
$(error WASM_BUILDDIR ($(W)) holds a character that make or the shell treats specially: name a build directory of its own, such as build-wasm)
endif
W_PHYSICAL := $(call physical,$(W))
ifeq ($(W_PHYSICAL),)
$(error WASM_BUILDDIR ($(W)) cannot be resolved: name a build directory of its own, such as build-wasm)
endif
ifneq ($(words $(W_PHYSICAL))$(call special,$(W_PHYSICAL)),1)
$(error WASM_BUILDDIR ($(W)) is $(W_PHYSICAL), which holds a blank or a character that make or the shell treats specially: name a build directory of its own, such as build-wasm)
endif
ifeq ($(W_PHYSICAL),/)
$(error WASM_BUILDDIR ($(W)) is /: name a build directory of its own, such as build-wasm)
endif
ifneq ($(HOME),)
ifeq ($(W_PHYSICAL),$(call physical,$(abspath $(HOME))))
$(error WASM_BUILDDIR ($(W)) is the home directory: name a build directory of its own, such as build-wasm)
endif
endif
SRC_PHYSICAL := $(call physical,$(SRCDIR))
ifneq ($(filter $(W_PHYSICAL) $(W_PHYSICAL)/%,$(SRC_PHYSICAL)),)
$(error WASM_BUILDDIR ($(W)) is the source tree or holds it: name a build directory of its own, such as build-wasm)
endif
ifneq ($(wildcard $(W_PHYSICAL)/CMakeLists.txt $(W_PHYSICAL)/.git),)
$(error WASM_BUILDDIR ($(W)) holds a source tree: name a build directory of its own, such as build-wasm)
endif
ifneq ($(wildcard $(W_PHYSICAL)/*),)
ifeq ($(wildcard $(W_PHYSICAL)/.make-wasm)$(findstring GENERATED BY make wasm,$(shell sed -n 1p '$(W_PHYSICAL)/config.make' 2>/dev/null)),)
ifneq ($(findstring /,$(patsubst $(SRC_PHYSICAL)/build-wasm%,%,$(W_PHYSICAL))),)
$(error WASM_BUILDDIR ($(W)) is not empty, and make wasm never built there: name a new or empty directory (or create $(W)/.make-wasm to build there anyway))
endif
endif
endif
WASM_BUILDDIR_CHECKED := 1
endef

ifneq ($(filter $(WASM_GOALS) $(W)/%,$(WASM_REQUESTED)),)
$(eval $(value WASM_BUILDDIR_CHECKS))
endif

# The first line of the recipes that write or remove in W
WASM_CHECK_BUILDDIR = $(if $(WASM_BUILDDIR_CHECKED),,$(eval $(value WASM_BUILDDIR_CHECKS)))

# emcmake comes with the Emscripten SDK: say so before the sources are
# generated, rather than once the wasm build is configured
ifneq ($(filter wasm wasm-check wasm-check-browser wasm-serve $(W)/cmake/%,$(WASM_REQUESTED)),)
ifeq ($(shell command -v $(firstword $(EMCMAKE)) 2>/dev/null),)
$(error emcmake ($(EMCMAKE)) not found: source emsdk_env.sh of the Emscripten SDK (see docs/WebAssembly.md), or set EMCMAKE)
endif
endif

# What an offline build is given must exist
$(foreach v,WASM_IMAGE_ZIP WASM_DEPS_DIR WASM_GENERATED WASM_VMMAKER_IMAGE WASM_VMMAKER_VM WASM_HOST_PHARO,\
  $(if $(and $($v),$(filter command line environment,$(origin $v))),\
    $(if $(wildcard $(abspath $($v))),,$(error $v: $(abspath $($v)) does not exist))))

# The VMMaker image and the VM running it: downloaded and bootstrapped in
# build-wasm/host (network needed), or a copy of WASM_VMMAKER_IMAGE and the
# directory holding it, and WASM_VMMAKER_VM.
ifneq ($(WASM_VMMAKER_IMAGE),)
VMMAKER_IMAGE_SOURCE := $(abspath $(WASM_VMMAKER_IMAGE))
VMMAKER_IMAGE := $(W)/host/vmmaker-image/$(notdir $(VMMAKER_IMAGE_SOURCE))
HOST_VMMAKER_FLAGS := -DGENERATE_VMMAKER=OFF "-DVMMAKER_IMAGE=$(VMMAKER_IMAGE)"
else
VMMAKER_IMAGE_SOURCE :=
VMMAKER_IMAGE := $(W)/host/build/vmmaker/image/VMMaker.image
HOST_VMMAKER_FLAGS := -DGENERATE_VMMAKER=ON
endif
ifneq ($(WASM_VMMAKER_VM),)
VMMAKER_VM := $(abspath $(WASM_VMMAKER_VM))
HOST_VMMAKER_FLAGS += "-DGENERATE_PHARO_VM=$(VMMAKER_VM)"
else
VMMAKER_VM := $(W)/host/build/vmmaker/vm/pharo
HOST_VMMAKER_FLAGS += -DGENERATE_PHARO_VM=
endif

# The generated sources, and the native Pharo VM that prepares the image of
# the world and runs the snapshot lane (the VMMaker VM by default)
ifeq ($(WASM_GENERATED),)
GEN := $(W)/host
GEN_STAMP := $(W)/host/.generated
WASM_HOST_PHARO ?= $(VMMAKER_VM)
else
GEN := $(abspath $(WASM_GENERATED))
GEN_STAMP :=
WASM_HOST_PHARO ?= $(if $(WASM_VMMAKER_VM),$(VMMAKER_VM),$(wildcard $(W)/host/build/vmmaker/vm/pharo))
endif
HOST_PHARO := $(if $(WASM_HOST_PHARO),$(abspath $(WASM_HOST_PHARO)))
IMAGE_ZIP := $(if $(WASM_IMAGE_ZIP),$(abspath $(WASM_IMAGE_ZIP)))
DEPS_DIR := $(if $(WASM_DEPS_DIR),$(abspath $(WASM_DEPS_DIR)))
BUILD_TYPE := $(if $(filter-out 0,$(WASM_DEBUG)),Debug,Release)

WASM_CMAKE_FLAGS = \
  -DWASM_SJLJ=$(WASM_SJLJ) \
  -DWASM_WEB_MEMORY64=$(WASM_WEB_MEMORY64) \
  -DWASM_STACK_SIZE=$(WASM_STACK_SIZE) \
  -DWASM_INITIAL_MEMORY=$(WASM_INITIAL_MEMORY) \
  -DWASM_MAXIMUM_MEMORY=$(WASM_MAXIMUM_MEMORY) \
  -DWASM_OLD_SPACE_BASE=$(WASM_OLD_SPACE_BASE) \
  -DWASM_SLICE_MS=$(WASM_SLICE_MS) \
  -DWASM_WORLD=$(WASM_WORLD) \
  -DWASM_FFI=$(WASM_FFI) \
  -DWASM_FREETYPE=$(WASM_FREETYPE) \
  -DWASM_CAIRO=$(WASM_CAIRO) \
  -DWASM_CAIRO_PDF=$(WASM_CAIRO_PDF) \
  -DWASM_LIBGIT2=$(WASM_LIBGIT2) \
  -DWASM_LIBGIT2_HTTP=$(WASM_LIBGIT2_HTTP) \
  -DWASM_SDL2=$(WASM_SDL2) \
  "-DWASM_IMAGE_ZIP=$(IMAGE_ZIP)" \
  "-DWASM_DEPS_DIR=$(DEPS_DIR)" \
  "-DWASM_HOST_PHARO=$(HOST_PHARO)" \
  -DCMAKE_BUILD_TYPE=$(BUILD_TYPE) \
  "-DGENERATED_SOURCE_DIR=$(GEN)" \
  "-DWASM_STAGE_DIR=$(W)" \
  "-DNODE_JS_EXECUTABLE=$(NODE)"

# Host command-line variables (CC=..., WASM_SLICE_MS=...) must not reach the
# Makefiles of CMake through MAKEFLAGS.
wasm wasm-check wasm-check-browser wasm-serve wasm-clean wasm-distclean: MAKEOVERRIDES :=

# cmake --build runs make, which shares the job slots of `make -jN'; without
# -j it runs WASM_JOBS jobs.  GNUMAKEFLAGS (GNU make 4.0 and later, and not
# Ninja) keeps the sub-makes from printing every directory they enter.
CMAKE_BUILD = case "$$MAKEFLAGS" in *-j*) set -- ;; *) set -- --parallel $(WASM_JOBS) ;; esac; \
  GNUMAKEFLAGS=--no-print-directory $(CMAKE) --build

wasm: $(GEN_STAMP) $(W)/cmake/.configured
	+$(CMAKE_BUILD) $(W)/cmake "$$@"

$(W)/.make-wasm:
	@$(WASM_CHECK_BUILDDIR)
	mkdir -p $(W)
	$(WASM_MARK)

# The recorded settings, rewritten (and so newer) only when one changed
$(W)/config.make: FORCE | $(W)/.make-wasm
	@$(WASM_CHECK_BUILDDIR)
	@{ echo "# GENERATED BY make wasm -- settings come from the make command line"; \
	  echo "EMCMAKE = $(EMCMAKE)"; \
	  echo "CMAKE = $(CMAKE)"; \
	  echo "NODE = $(NODE)"; \
	  echo "WASM_DEBUG = $(WASM_DEBUG)"; \
	  echo "WASM_SJLJ = $(WASM_SJLJ)"; \
	  echo "WASM_WEB_MEMORY64 = $(WASM_WEB_MEMORY64)"; \
	  echo "WASM_STACK_SIZE = $(WASM_STACK_SIZE)"; \
	  echo "WASM_INITIAL_MEMORY = $(WASM_INITIAL_MEMORY)"; \
	  echo "WASM_MAXIMUM_MEMORY = $(WASM_MAXIMUM_MEMORY)"; \
	  echo "WASM_OLD_SPACE_BASE = $(WASM_OLD_SPACE_BASE)"; \
	  echo "WASM_SLICE_MS = $(WASM_SLICE_MS)"; \
	  echo "WASM_WORLD = $(WASM_WORLD)"; \
	  echo "WASM_FFI = $(WASM_FFI)"; \
	  echo "WASM_FREETYPE = $(WASM_FREETYPE)"; \
	  echo "WASM_CAIRO = $(WASM_CAIRO)"; \
	  echo "WASM_CAIRO_PDF = $(WASM_CAIRO_PDF)"; \
	  echo "WASM_LIBGIT2 = $(WASM_LIBGIT2)"; \
	  echo "WASM_LIBGIT2_HTTP = $(WASM_LIBGIT2_HTTP)"; \
	  echo "WASM_SDL2 = $(WASM_SDL2)"; \
	  echo "WASM_IMAGE_ZIP = $(IMAGE_ZIP)"; \
	  echo "WASM_DEPS_DIR = $(DEPS_DIR)"; \
	  echo "WASM_GENERATED = $(if $(WASM_GENERATED),$(GEN))"; \
	  echo "WASM_HOST_PHARO = $(HOST_PHARO)"; } >$@.tmp
	@if cmp -s $@.tmp $@; then rm $@.tmp; else mv $@.tmp $@ && echo "make wasm: settings recorded in $@"; fi

$(W)/config-host.make: FORCE | $(W)/.make-wasm
	@$(WASM_CHECK_BUILDDIR)
	@{ echo "# GENERATED BY make wasm -- the VMMaker of build-wasm/host"; \
	  echo "WASM_VMMAKER_IMAGE = $(VMMAKER_IMAGE_SOURCE)"; \
	  echo "WASM_VMMAKER_VM = $(if $(WASM_VMMAKER_VM),$(VMMAKER_VM))"; } >$@.tmp
	@if cmp -s $@.tmp $@; then rm $@.tmp; else mv $@.tmp $@ && echo "make wasm: settings recorded in $@"; fi

# The wasm build: configured again when a setting or the initial cache
# changes.  (CMake configures it again itself when a plugin of
# src/emscripten/plugins, or a class of the OSWindow-Web package, comes or
# goes.)  The knobs come before -C, so that the initial cache sees them.
# (The stamps of the two trees are touched only once CMake succeeded.)
$(W)/cmake/.configured: $(W)/config.make $(SRCDIR)/cmake/Emscripten.cache.cmake | $(GEN_STAMP)
	$(EMCMAKE) $(CMAKE) -S $(SRCDIR) -B $(W)/cmake $(WASM_CMAKE_FLAGS) -C $(SRCDIR)/cmake/Emscripten.cache.cmake
	touch $@

# Generation of the sources, in the native host tree, which builds nothing
# else (so needs neither libffi, OpenSSL nor libuuid).  After the first
# generation, a newer .st file, or one that came or went (which changes the
# list of them, host/.smalltalk-sources), makes it reload this tree's
# smalltalksrc into the VMMaker image (scripts/refreshVMMaker.st), whose
# generate command then runs again.  So does a VMMaker image copied from
# WASM_VMMAKER_IMAGE, which may come from an older tree.  (The vmmaker
# target does not build vmmaker_vm when the image is given.)
ifeq ($(WASM_GENERATED),)
SMALLTALK_SOURCES := $(shell find $(SRCDIR)/smalltalksrc -name '*.st')
SMALLTALK_SOURCE_LIST := $(W)/host/.smalltalk-sources
REFRESH_VMMAKER := $(W)/host/.refresh-vmmaker

$(SMALLTALK_SOURCE_LIST): FORCE | $(W)/.make-wasm
	@$(WASM_CHECK_BUILDDIR)
	@mkdir -p $(W)/host
	@find $(SRCDIR)/smalltalksrc -name '*.st' | LC_ALL=C sort >$@.tmp
	@if cmp -s $@.tmp $@; then rm $@.tmp; else mv $@.tmp $@; fi

$(W)/host/.configured: $(W)/config-host.make
	$(CMAKE) -S $(SRCDIR) -B $(W)/host -DFLAVOUR=StackVM -DGENERATE_SOURCES=ON -DFEATURE_FFI=OFF \
	  -DBUILD_BUNDLE=OFF -DFEATURE_LIB_SDL2=OFF -DFEATURE_LIB_CAIRO=OFF -DFEATURE_LIB_FREETYPE2=OFF \
	  -DFEATURE_LIB_GIT2=OFF -DFEATURE_PLUGIN_SSL=OFF -DFEATURE_PLUGIN_UUID=OFF -DBUILD_WITH_GRAPHVIZ=OFF \
	  -DICEBERG_DEFAULT_REMOTE=httpsUrl $(HOST_VMMAKER_FLAGS)
	touch $@

ifneq ($(VMMAKER_IMAGE_SOURCE),)
$(VMMAKER_IMAGE): $(VMMAKER_IMAGE_SOURCE) $(W)/config-host.make
	rm -rf $(W)/host/vmmaker-image
	mkdir -p $(W)/host/vmmaker-image
	cp -R $(dir $(VMMAKER_IMAGE_SOURCE)). $(W)/host/vmmaker-image/
	touch $(REFRESH_VMMAKER) $@
endif

$(W)/host/.generated: $(W)/host/.configured $(if $(VMMAKER_IMAGE_SOURCE),$(VMMAKER_IMAGE)) \
  $(SMALLTALK_SOURCE_LIST) $(SMALLTALK_SOURCES)
	+$(CMAKE_BUILD) $(W)/host --target vmmaker_vm "$$@"
	+$(CMAKE_BUILD) $(W)/host --target vmmaker "$$@"
	if test -f $(REFRESH_VMMAKER) $(if $(filter %.st $(SMALLTALK_SOURCE_LIST),$?),|| test -f $@); then \
	  cd $(dir $(VMMAKER_IMAGE)) && $(VMMAKER_VM) --headless $(VMMAKER_IMAGE) --no-default-preferences \
	    --save --quit $(SRCDIR)/scripts/refreshVMMaker.st $(SRCDIR) && rm -f $(REFRESH_VMMAKER); \
	fi
	+$(CMAKE_BUILD) $(W)/host --target generate-sources "$$@"
	touch $@
endif

# Tests

wasm-check: wasm
	env NODE="$(NODE)" WASM_DIR="$(W)" GEN="$(GEN)/generated/64" HOST_PHARO="$(HOST_PHARO)" \
	  SRCDIR="$(SRCDIR)" TEST_DIR="$(W)/tests-run" sh $(SRCDIR)/tests/wasm/run-lanes.sh

# Playwright is no dependency of the build: the specs load PLAYWRIGHT_MODULE,
# or the playwright package that node finds from tests/wasm/lib, and the goal
# fails without either.  world.spec.mjs needs the image of the world, which a
# build without one (WASM_WORLD=OFF, or no host Pharo) skips, as lane 80 does,
# ffi.spec.mjs the FFI, which a build without it (WASM_FFI=OFF) skips,
# git.spec.mjs libgit2, which only a build with WASM_LIBGIT2=ON has, and
# sdl.spec.mjs SDL2 and its page sdl.html, which only a build with
# WASM_SDL2=ON has.
# web/manifest.json says what the build has.
wasm-check-browser: wasm
	@if test -z "$$PLAYWRIGHT_MODULE" && ! (cd $(SRCDIR)/tests/wasm/lib && \
	    $(NODE) -e 'require.resolve("playwright")') >/dev/null 2>&1; then \
	  echo "make wasm-check-browser: the browser specs need Playwright: set PLAYWRIGHT_MODULE (and BROWSERS), e.g." >&2; \
	  echo "  PLAYWRIGHT_MODULE=/path/to/node_modules/playwright BROWSERS=chromium,firefox make wasm-check-browser" >&2; \
	  echo "(the specs are tests/wasm/page.spec.mjs, world.spec.mjs, ffi.spec.mjs, git.spec.mjs and sdl.spec.mjs)" >&2; \
	  exit 1; \
	fi
	$(NODE) $(SRCDIR)/tests/wasm/page.spec.mjs $(W)/web
	@if grep -q '"world": *true' $(W)/web/manifest.json; then \
	  echo "$(NODE) $(SRCDIR)/tests/wasm/world.spec.mjs $(W)/web"; \
	  $(NODE) $(SRCDIR)/tests/wasm/world.spec.mjs $(W)/web; \
	else \
	  echo "skip world.spec.mjs: $(W)/web/manifest.json has no world image (WASM_WORLD=OFF, or no host Pharo to prepare it)"; \
	fi
	@if grep -q '"ffi": *true' $(W)/web/manifest.json; then \
	  echo "$(NODE) $(SRCDIR)/tests/wasm/ffi.spec.mjs $(W)/web"; \
	  $(NODE) $(SRCDIR)/tests/wasm/ffi.spec.mjs $(W)/web; \
	else \
	  echo "skip ffi.spec.mjs: $(W)/web/manifest.json has no FFI (WASM_FFI=OFF)"; \
	fi
	@if grep -q '"git": *true' $(W)/web/manifest.json; then \
	  echo "$(NODE) $(SRCDIR)/tests/wasm/git.spec.mjs $(W)/web"; \
	  $(NODE) $(SRCDIR)/tests/wasm/git.spec.mjs $(W)/web; \
	else \
	  echo "skip git.spec.mjs: $(W)/web/manifest.json has no libgit2 (WASM_LIBGIT2=OFF, the default, or WASM_FFI=OFF)"; \
	fi
	@if grep -q '"sdl2": *true' $(W)/web/manifest.json; then \
	  echo "$(NODE) $(SRCDIR)/tests/wasm/sdl.spec.mjs $(W)/web"; \
	  $(NODE) $(SRCDIR)/tests/wasm/sdl.spec.mjs $(W)/web; \
	else \
	  echo "skip sdl.spec.mjs: $(W)/web/manifest.json has no SDL2 (WASM_SDL2=OFF, the default, or WASM_FFI=OFF)"; \
	fi

wasm-serve: wasm
	$(NODE) $(SRCDIR)/packaging/emscripten/tools/serve.mjs $(W)/web $(WASM_PORT)

# Cleaning never removes build-wasm itself, whatever else it holds, nor its
# .make-wasm, which wasm-distclean first writes in a directory that only the
# config.make of an earlier make wasm marks.

wasm-clean:
	@$(WASM_CHECK_BUILDDIR)
	rm -rf $(W)/cmake $(W)/node $(W)/web $(W)/tests-run

wasm-distclean: wasm-clean
	@$(WASM_CHECK_BUILDDIR)
	test -f $(W)/.make-wasm || ! test -f $(W)/config.make || $(WASM_MARK)
	rm -rf $(W)/host $(W)/image $(W)/config.make $(W)/config-host.make
ifneq ($(filter-out 0,$(WASM_CLEAN_DOWNLOADS)),)
	rm -rf $(W)/downloads
endif

FORCE:
