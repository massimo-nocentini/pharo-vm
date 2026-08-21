
CMAKE=`brew --prefix cmake`/bin/cmake

rm -rf build
${CMAKE} -S . -B build -DUSE_RUST_PLATFORM=ON -DUSE_RUST_PLUGINS=OFF -DALWAYS_INTERACTIVE=TRUE -DPHARO_DEPENDENCIES_PREFER_DOWNLOAD_BINARIES=TRUE -DCMAKE_C_COMPILER=${CC} -DCMAKE_CXX_COMPILER=${CXX} -DICEBERG_DEFAULT_REMOTE=httpsUrl
${CMAKE} --build build
${CMAKE} --install build
rm -rf /Applications/Pharo.app
cp -r build/build/dist/Pharo.app /Applications
rm -rf build