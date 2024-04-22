
wget https://www.lua.org/ftp/lua-5.4.6.tar.gz
tar xfz lua-5.4.6.tar.gz
cd lua-5.4.6
make "MYCFLAGS=-fPIC" linux
sudo make install
cd ..

git clone --depth 1 https://github.com/massimo-nocentini/datetimeformatter.c.git
cd datetimeformatter.c/src
make && sudo make install
cd ../../

git clone --depth 1 https://github.com/massimo-nocentini/timsort.c.git
cd timsort.c/src
make && sudo make install
cd ../../

mkdir pharo-vm-c-src
cd pharo-vm-c-src
wget https://files.pharo.org/vm/pharo-spur64-headless/Linux-x86_64/source/PharoVM-10.2.0-f4c5e2a-Linux-x86_64-c-src.zip
unzip PharoVM-10.1.1-92bf0d3-Linux-x86_64-c-src.zip
cd ..

cmake -S pharo-vm -B pharo-vm-build -DPHARO_DEPENDENCIES_PREFER_DOWNLOAD_BINARIES=TRUE -DBUILD_IS_RELEASE=ON -DICEBERG_DEFAULT_REMOTE=httpsUrl -DGENERATE_SOURCES=FALSE -DGENERATED_SOURCE_DIR=../pharo-vm-c-src/pharo-vm/
cmake --build pharo-vm-build --target install
rm -rf build/ pharo-vm-build/build/dist/lib/{libss*,libcairo.so*,libgit2.*,libharfbuzz.so*,libfontconfig.so*} #,libbz2*,libexpat*,libffi*,libfreetype*,libpixman*,libpng*"

cp /usr/lib/x86_64-linux-gnu/{libssh2.so,libssl.so,libcairo-gobject.so,libcairo.so,libpango-1.0.so,libpangocairo-1.0.so,libgit2-glib-1.0.so,libgit2.so,libharfbuzz-cairo.so,libharfbuzz-gobject.so,libharfbuzz-icu.so,libharfbuzz.so,libharfbuzz-subset.so,libfontconfig.so} /usr/local/lib/liblua.a pharo-vm-build/build/dist/lib/


cd pharo-vm-build/build/dist/
zip -r pharo-vm-ubuntu.zip *