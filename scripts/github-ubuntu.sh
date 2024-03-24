

git clone --depth 1 https://github.com/massimo-nocentini/datetimeformatter.c.git
cd datetimeformatter.c/src
make && sudo make install
cd ..

git clone --depth 1 https://github.com/massimo-nocentini/timsort.c.git
cd timsort.c/src
make && sudo make install
cd ..

wget https://files.pharo.org/vm/pharo-spur64-headless/Linux-x86_64/source/PharoVM-10.1.1-32b2be5-Linux-x86_64-c-src.zip

unzip PharoVM-10.1.1-32b2be5-Linux-x86_64-c-src.zip



#export CC=clang && export CXX=clang++
cmake -S pharo-vm -B pharo-vm-build -DPHARO_DEPENDENCIES_PREFER_DOWNLOAD_BINARIES=TRUE -DBUILD_IS_RELEASE=ON -DICEBERG_DEFAULT_REMOTE=httpsUrl -DGENERATE_SOURCES=FALSE -DGENERATED_SOURCE_DIR=../pharo-vm-c-src/pharo-vm/
