
export PATH=/c/msys64/usr/local/bin:/c/msys64/usr/local/lib:/c/msys64/ucrt64/bin:/c/msys64/usr/bin:$PATH

CURRENT_DIR=`pwd`

cd ../

cp /usr/local/include/lua.h /usr/local/include/lauxlib.h /usr/local/include/luaconf.h /usr/local/include/lualib.h current-dependencies/include
cp /usr/local/lib/lua54.dll current-dependencies/lib

cd ../datetimeformatter.c/src
make mingw
cp datetimeformatter.h ${CURRENT_DIR}/../current-dependencies/include
cp libdatetimeformatter.dll ${CURRENT_DIR}/../current-dependencies/lib
cd ../

cd ../timsort.c/src
make mingw
cp timsort.h ${CURRENT_DIR}/../current-dependencies/include
cp libtimsort.dll ${CURRENT_DIR}/../current-dependencies/lib
cd ../

cd ../pharo-vm-compiling

rm -rf pharo-vm-c-src
mkdir -p pharo-vm-c-src
cd pharo-vm-c-src
wget http://files.pharo.org/vm/pharo-spur64-headless/Windows-x86_64/source/PharoVM-10.0.9-de760673-Windows-x86_64-c-src.zip
unzip PharoVM-10.0.9-de760673-Windows-x86_64-c-src.zip
cd ../

cmake -S pharo-vm -B pharo-vm-build -DUSE_MSYS2=TRUE -DPHARO_DEPENDENCIES_PREFER_DOWNLOAD_BINARIES=TRUE -DBUILD_IS_RELEASE=ON -DICEBERG_DEFAULT_REMOTE=httpsUrl -DGENERATE_SOURCES=FALSE -DGENERATED_SOURCE_DIR=../pharo-vm-c-src/pharo-vm/