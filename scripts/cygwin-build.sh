
set -e

cd ../

# cp -r /cygdrive/c/msys64/usr/local/include /usr/local
# cp -r /cygdrive/c/msys64/usr/local/lib /usr/local


cmake --build pharo-vm-build --target install
rm -rf pharo-vm-dist
cp -r pharo-vm-build/build/dist pharo-vm-dist

chmod +w pharo-vm-dist/*.dll    # necessary for the copyings that follows.

cp /usr/local/lib/*.dll pharo-vm-dist/
echo "Copied usr local dlls."

cp gtk3/ucrt64/bin/*.dll pharo-vm-dist/
echo "Copied Gtk dlls."