
wget https://www.lua.org/ftp/lua-5.4.7.tar.gz --no-verbose
tar xfz lua-5.4.7.tar.gz
cd lua-5.4.7
make "MYCFLAGS=-fPIC" linux
sudo make install
cd ..

mkdir nodejs
cd nodejs
wget https://github.com/massimo-nocentini/ci.github/releases/latest/download/ubuntu-node-v22.1.0.zip --no-verbose
unzip ubuntu-node-v22.1.0.zip
sudo cp bin/* /usr/local/bin/
sudo cp -r lib/* /usr/local/lib/
sudo cp -r include/* /usr/local/include/
cd ..

git clone --depth 1 https://github.com/massimo-nocentini/datetimeformatter.c.git
cd datetimeformatter.c/src
make && sudo make install
cd ../../

git clone --depth 1 https://github.com/massimo-nocentini/timsort.c.git
cd timsort.c/src
make && sudo make install
cd ../../

mkdir tree-sitter
cd tree-sitter

git clone --depth 1 https://github.com/tree-sitter/tree-sitter.git
cd tree-sitter/
make && sudo make install
cd ../

wget https://github.com/tree-sitter/tree-sitter/releases/latest/download/tree-sitter-linux-x64.gz --no-verbose
gunzip -d -k -r -v tree-sitter-linux-x64
chmod +x tree-sitter-linux-x64
sudo cp tree-sitter-linux-x64 /usr/local/bin/tree-sitter

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

# mkdir pharo-vm-c-src
# cd pharo-vm-c-src
# wget https://files.pharo.org/vm/pharo-spur64-headless/Linux-x86_64/source/PharoVM-10.2.1-d1bfe9e-Linux-x86_64-c-src.zip --no-verbose
# unzip PharoVM-10.2.1-d1bfe9e-Linux-x86_64-c-src.zip
# cd ..

cmake -S pharo-vm -B pharo-vm-build -DPHARO_DEPENDENCIES_PREFER_DOWNLOAD_BINARIES=TRUE -DBUILD_IS_RELEASE=ON -DICEBERG_DEFAULT_REMOTE=httpsUrl -DGENERATE_SOURCES=TRUE #-DGENERATED_SOURCE_DIR=../pharo-vm-c-src/pharo-vm/
cmake --build pharo-vm-build --target install
rm -rf build/ pharo-vm-build/build/dist/lib/{libss*,libcairo.so*,libgit2.*,libharfbuzz.so*,libfontconfig.so*} #,libbz2*,libexpat*,libffi*,libfreetype*,libpixman*,libpng*"

cp /usr/lib/x86_64-linux-gnu/{libssh2.so,libssl.so,libcairo-gobject.so,libcairo.so,libpango-1.0.so,libpangocairo-1.0.so,libgit2-glib-1.0.so,libgit2.so,libharfbuzz-cairo.so,libharfbuzz-gobject.so,libharfbuzz-icu.so,libharfbuzz.so,libharfbuzz-subset.so,libfontconfig.so} pharo-vm-build/build/dist/lib/
cp /usr/local/lib/{liblua.a,libtree-sitter.so,libtree-sitter-c.so,libtree-sitter-json.so,libtree-sitter-javascript.so,libtree-sitter-python.so} pharo-vm-build/build/dist/lib/
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
zip -r pharo-vm-ubuntu.zip *

cd ../../../

mkdir booklet-image
cd booklet-image

curl https://get.pharo.org/64/130 | bash

../pharo-vm-build/build/dist/pharo Pharo.image eval "

remote := IceGitRemote
	          name: 'mn'
	          url: 'https://github.com/massimo-nocentini/pharo.git'.

repo := IceRepository repositoryNamed: 'pharo'.

IceRepositoryCreator new
	repository: repo;
	remote: remote;
	location: repo location;
	createRepository.

repo checkoutBranch: 'primitives-for-athens-cairo-canvas-merge'.

[ Metacello new
    baseline: 'BookletDSst';
    repository: 'github://massimo-nocentini/Booklet-DSst/src';
    load ] on: MCMergeOrLoadWarning do: [:warning | warning load ].

Smalltalk snapshot: true andQuit: true."

rm -rf pharo-local/iceberg
zip -r Pharo130-booklet.zip *

cd ..