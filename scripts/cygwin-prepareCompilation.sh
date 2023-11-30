
set -e

cd ../ 

rm -rf pharo-vm-c-src
mkdir -p pharo-vm-c-src
cd pharo-vm-c-src
wget http://files.pharo.org/vm/pharo-spur64-headless/Windows-x86_64/source/PharoVM-10.0.9-de760673-Windows-x86_64-c-src.zip
unzip PharoVM-10.0.9-de760673-Windows-x86_64-c-src.zip
cd ../
cmake -S pharo-vm -B pharo-vm-build -DPHARO_DEPENDENCIES_PREFER_DOWNLOAD_BINARIES=TRUE -DBUILD_IS_RELEASE=ON -DICEBERG_DEFAULT_REMOTE=httpsUrl -DGENERATE_SOURCES=FALSE -DGENERATED_SOURCE_DIR=../pharo-vm-c-src/pharo-vm/
			