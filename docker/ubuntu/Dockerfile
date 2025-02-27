

FROM ubuntu:latest

RUN apt update && apt upgrade -y && apt install -y build-essential cmake git wget clang curl tree-sitter-cli libtree-sitter-dev libbz2-dev libexpat1-dev libffi-dev libfreetype-dev libpixman-1-dev libpng-dev libsdl2-dev libsdl2-gfx-dev libsdl2-image-dev libsdl2-mixer-dev libsdl2-net-dev libsdl2-ttf-dev libpango1.0-dev libpangocairo-1.0-0 libpangoft2-1.0-0 libpangoxft-1.0-0 libgit2-dev libffi-dev libffindex0-dev libtty-dev libpoppler-dev libpoppler-glib-dev libgit2-glib-1.0-dev libssh2-1-dev openssl libgdk-pixbuf-2.0-dev libgtk-4-dev

ENV CC=clang
ENV CXX=clang++

RUN wget https://www.lua.org/ftp/lua-5.4.7.tar.gz --no-verbose && tar -xzf lua-5.4.7.tar.gz && \
    cd lua-5.4.7 && make CC=clang MYCFLAGS="-fPIC" && make install && cd .. && rm -rf lua-5.4.7

WORKDIR /home/ubuntu/tree-sitter

RUN git clone https://github.com/massimo-nocentini/datetimeformatter.c.git --depth=1 && \
    cd datetimeformatter.c/src && make linux && make install && cd ../.. && rm -rf datetimeformatter.c

RUN git clone --depth 1 https://github.com/tree-sitter/tree-sitter-c.git && \
    cd tree-sitter-c && tree-sitter generate && make && make install

RUN git clone --depth 1 https://github.com/tree-sitter/tree-sitter-json.git && \
    cd tree-sitter-json && tree-sitter generate && make && make install

RUN git clone --depth 1 https://github.com/tree-sitter/tree-sitter-javascript.git && \
    cd tree-sitter-javascript && tree-sitter generate && make && make install

RUN git clone --depth 1 https://github.com/tree-sitter/tree-sitter-python.git && \
    cd tree-sitter-python && tree-sitter generate && make && make install

WORKDIR /home/ubuntu

COPY . pharo-vm

RUN cmake -S pharo-vm -B pharo-vm-build -DVERSION_UPDATE_FROM_GIT=FALSE -DPHARO_DEPENDENCIES_PREFER_DOWNLOAD_BINARIES=TRUE -DICEBERG_DEFAULT_REMOTE=httpsUrl -DGENERATE_SOURCES=TRUE
RUN cmake --build pharo-vm-build --target install

RUN mkdir -p pharo-vm-build/build/dist/share/tree-sitter/language && \
    mkdir pharo-vm-build/build/dist/share/tree-sitter/language/c && \
    mkdir pharo-vm-build/build/dist/share/tree-sitter/language/json && \
    mkdir pharo-vm-build/build/dist/share/tree-sitter/language/javascript && \
    mkdir pharo-vm-build/build/dist/share/tree-sitter/language/python && \
    cp tree-sitter/tree-sitter-c/grammar.js pharo-vm-build/build/dist/share/tree-sitter/language/c/ && \
    cp -r tree-sitter/tree-sitter-c/queries/ pharo-vm-build/build/dist/share/tree-sitter/language/c/ && \
    cp tree-sitter/tree-sitter-json/grammar.js pharo-vm-build/build/dist/share/tree-sitter/language/json/ && \
    cp -r tree-sitter/tree-sitter-json/queries/ pharo-vm-build/build/dist/share/tree-sitter/language/json/ && \
    cp tree-sitter/tree-sitter-javascript/grammar.js pharo-vm-build/build/dist/share/tree-sitter/language/javascript/ && \
    cp -r tree-sitter/tree-sitter-javascript/queries/ pharo-vm-build/build/dist/share/tree-sitter/language/javascript/ && \
    cp tree-sitter/tree-sitter-python/grammar.js pharo-vm-build/build/dist/share/tree-sitter/language/python/ && \
    cp -r tree-sitter/tree-sitter-python/queries/ pharo-vm-build/build/dist/share/tree-sitter/language/python/

RUN rm -rf tree-sitter pharo-vm build && cp -r pharo-vm-build/build pharo-vm && rm -rf pharo-vm-build 
