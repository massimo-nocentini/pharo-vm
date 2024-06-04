
CURRENT_DIR=$(pwd)

cd $CURRENT_DIR

mkdir current-dependencies
mkdir current-dependencies/include
mkdir current-dependencies/lib

wget https://www.lua.org/ftp/lua-5.4.6.tar.gz
tar xfz lua-5.4.6.tar.gz
cd lua-5.4.6
mingw32-make mingw
cd src
cp lua.exe luac.exe lua54.dll /ucrt64/bin/
cp ./{lua.h,lauxlib.h,luaconf.h,lualib.h} /ucrt64/include/
cp lua54.dll /ucrt64/lib/

cp ./{lua.h,lauxlib.h,luaconf.h,lualib.h} ${CURRENT_DIR}/current-dependencies/include
cp lua54.dll ${CURRENT_DIR}/current-dependencies/lib
cd ../../

git clone --depth 1 https://github.com/massimo-nocentini/datetimeformatter.c.git
cd datetimeformatter.c/src
mingw32-make mingw
cp datetimeformatter.h ${CURRENT_DIR}/current-dependencies/include
cp libdatetimeformatter.dll ${CURRENT_DIR}/current-dependencies/lib
cd ../../

git clone --depth 1 https://github.com/massimo-nocentini/timsort.c.git
cd timsort.c/src
mingw32-make mingw
cp timsort.h ${CURRENT_DIR}/current-dependencies/include
cp libtimsort.dll ${CURRENT_DIR}/current-dependencies/lib
cd ../../

mkdir tree-sitter
cd tree-sitter

git clone --depth 1 https://github.com/tree-sitter/tree-sitter-c.git
cd tree-sitter-c
tree-sitter generate
tree-sitter build
cp c.dll ${CURRENT_DIR}/current-dependencies/lib/libtree-sitter-c.dll
cd ../

git clone --depth 1 https://github.com/tree-sitter/tree-sitter-json.git
cd tree-sitter-json
tree-sitter generate
tree-sitter build
cp json.dll ${CURRENT_DIR}/current-dependencies/lib/libtree-sitter-json.dll
cd ../

git clone --depth 1 https://github.com/tree-sitter/tree-sitter-javascript.git
cd tree-sitter-javascript
tree-sitter generate
tree-sitter build
cp javascript.dll ${CURRENT_DIR}/current-dependencies/lib/libtree-sitter-javascript.dll
cd ../

git clone --depth 1 https://github.com/tree-sitter/tree-sitter-python.git
cd tree-sitter-python
tree-sitter generate
tree-sitter generate
tree-sitter build
cp python.dll ${CURRENT_DIR}/current-dependencies/lib/libtree-sitter-python.dll
cd ../

cp -r /ucrt64/include/tree_sitter/ ${CURRENT_DIR}/current-dependencies/include
cp /ucrt64/bin/libtree-sitter.dll ${CURRENT_DIR}/current-dependencies/lib

cd ../  # out of tree-sitter

rm -rf pharo-vm-c-src
mkdir -p pharo-vm-c-src
cd pharo-vm-c-src
wget https://files.pharo.org/vm/pharo-spur64-headless/Windows-x86_64/source/PharoVM-10.2.1-d1bfe9ec-Windows-x86_64-c-src.zip
unzip PharoVM-10.2.1-d1bfe9ec-Windows-x86_64-c-src.zip
cd ../

cmake -S pharo-vm -B pharo-vm-build -DUSE_MSYS2=TRUE -DPHARO_DEPENDENCIES_PREFER_DOWNLOAD_BINARIES=TRUE -DBUILD_IS_RELEASE=ON -DICEBERG_DEFAULT_REMOTE=httpsUrl -DGENERATE_SOURCES=FALSE -DGENERATED_SOURCE_DIR=../pharo-vm-c-src/pharo-vm/

cmake --build pharo-vm-build --target install

chmod +w pharo-vm-build/build/dist/*.dll    # necessary for the copyings that follows.

git clone --depth 1 https://github.com/massimo-nocentini/msys2-fetcher.lua.git
cd msys2-fetcher.lua/test
bash pharo-extension.sh
cd ../../

cp current-dependencies/lib/*.dll pharo-vm-build/build/dist/
cp msys2-fetcher.lua/test/pharo-goodies-sandbox/temp/ucrt64/bin/*.dll pharo-vm-build/build/dist/

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

cd pharo-vm-build/build/dist/

zip -r pharo-vm-windows.zip *