

wget https://www.lua.org/ftp/lua-5.4.7.tar.gz --no-verbose
tar xfz lua-5.4.7.tar.gz
cd lua-5.4.7
make "MYCFLAGS=-fPIC" macosx
sudo make install
cd ..

git clone --depth 1 https://github.com/massimo-nocentini/datetimeformatter.c.git
cd datetimeformatter.c/src
make macos && sudo make install-macos
cd ../../

mkdir tree-sitter
cd tree-sitter

git clone --depth 1 https://github.com/tree-sitter/tree-sitter-c.git
cd tree-sitter-c
tree-sitter generate
make && sudo make install
cd ../

git clone --depth 1 https://github.com/tree-sitter/tree-sitter-json.git
cd tree-sitter-json
tree-sitter generate
make && sudo make install
cd ../

git clone --depth 1 https://github.com/tree-sitter/tree-sitter-javascript.git
cd tree-sitter-javascript
tree-sitter generate
make && sudo make install
cd ../

git clone --depth 1 https://github.com/tree-sitter/tree-sitter-python.git
cd tree-sitter-python
tree-sitter generate
tree-sitter generate
make && sudo make install
cd ../

cd ../

# _____________________________________________________________________________________________________________________________________________
# For the sake of documentation, the following commands download the Pharo VM sources 
# from Pharo upstream to avoid generation of the C sources,
# which is faster but not indipedent from the Pharo's team build process.

# mkdir pharo-vm-c-src
# cd pharo-vm-c-src
# wget https://files.pharo.org/vm/pharo-spur64-headless/Darwin-x86_64/source/PharoVM-10.2.1-d1bfe9ec-Darwin-x86_64-c-src.zip --no-verbose
# unzip PharoVM-10.2.1-d1bfe9ec-Darwin-x86_64-c-src.zip
# cd ..
# _____________________________________________________________________________________________________________________________________________

export CC=$(brew --prefix llvm)/bin/clang
export CXX=$(brew --prefix llvm)/bin/clang++

cmake -S pharo-vm -B pharo-vm-build \
    -DALWAYS_INTERACTIVE=TRUE \
    -DPHARO_DEPENDENCIES_PREFER_DOWNLOAD_BINARIES=TRUE \
    -DBUILD_IS_RELEASE=ON \
    -DICEBERG_DEFAULT_REMOTE=httpsUrl \
    -DGENERATE_SOURCES=TRUE #-DGENERATED_SOURCE_DIR=../pharo-vm-c-src/pharo-vm/

# Copy SDL2 library to the build directory to replace the downloaded ones.
cp $(brew --prefix sdl2)/lib/libSDL2-2.0.0.dylib pharo-vm-build/SDL2-2.24.1-src/
cp $(brew --prefix sdl2)/lib/libSDL2-2.0.0.dylib pharo-vm-build/SDL2-2.24.1-src/libSDL2.dylib

cmake --build pharo-vm-build --target install
#rm -rf build/ pharo-vm-build/build/dist/lib/{libss*,libcairo.so*,libgit2.*,libharfbuzz.so*,libfontconfig.so*} #,libbz2*,libexpat*,libffi*,libfreetype*,libpixman*,libpng*"

# copy now up to date libraries:
cp $(brew --prefix cairo)/lib/*.dylib pharo-vm-build/build/dist/lib/
cp $(brew --prefix fontconfig)/lib/*.dylib pharo-vm-build/build/dist/lib/
cp $(brew --prefix freetype)/lib/*.dylib pharo-vm-build/build/dist/lib/
cp $(brew --prefix gdk-pixbuf)/lib/*.dylib pharo-vm-build/build/dist/lib/
cp $(brew --prefix gobject-introspection)/lib/*.dylib pharo-vm-build/build/dist/lib/
cp $(brew --prefix gtk4)/lib/*.dylib pharo-vm-build/build/dist/lib/
cp $(brew --prefix harfbuzz)/lib/*.dylib pharo-vm-build/build/dist/lib/
cp $(brew --prefix glib)/lib/*.dylib pharo-vm-build/build/dist/lib/
cp $(brew --prefix pixman)/lib/*.dylib pharo-vm-build/build/dist/lib/

# copy libraries that we've compiled.
cp /usr/local/lib/{liblua.a,libdatetimeformatter.dylib,libtree-sitter.dylib,libtree-sitter-c.dylib,libtree-sitter-json.dylib,libtree-sitter-javascript.dylib,libtree-sitter-python.dylib} pharo-vm-build/build/dist/lib/

# `tree-sitter` stuff.
mkdir -p pharo-vm-build/build/dist/share/tree-sitter/language
mkdir pharo-vm-build/build/dist/share/tree-sitter/language/c
mkdir pharo-vm-build/build/dist/share/tree-sitter/language/json
mkdir pharo-vm-build/build/dist/share/tree-sitter/language/javascript
mkdir pharo-vm-build/build/dist/share/tree-sitter/language/python

cp tree-sitter/tree-sitter-c/grammar.js pharo-vm-build/build/dist/share/tree-sitter/language/c/
cp -r tree-sitter/tree-sitter-c/queries/ pharo-vm-build/build/dist/share/tree-sitter/language/c/

cp tree-sitter/tree-sitter-json/grammar.js pharo-vm-build/build/dist/share/tree-sitter/language/json/
cp -r tree-sitter/tree-sitter-json/queries/ pharo-vm-build/build/dist/share/tree-sitter/language/json/

cp tree-sitter/tree-sitter-javascript/grammar.js pharo-vm-build/build/dist/share/tree-sitter/language/javascript/
cp -r tree-sitter/tree-sitter-javascript/queries/ pharo-vm-build/build/dist/share/tree-sitter/language/javascript/

cp tree-sitter/tree-sitter-python/grammar.js pharo-vm-build/build/dist/share/tree-sitter/language/python/
cp -r tree-sitter/tree-sitter-python/queries/ pharo-vm-build/build/dist/share/tree-sitter/language/python/

# Almost done, zipping.
cd pharo-vm-build/build/dist/
zip -r pharo-vm-macos.zip *

echo "Done"