
set -e

cd ../ 
cmake --build pharo-vm-build --target install
rm -rf pharo-vm-dist
cp -r pharo-vm-build/build/dist pharo-vm-dist
chmod +w pharo-vm-dist/*.dll
cp ucrt64-gtk3/bin/*.dll pharo-vm-dist/
echo "Copied Gtk dlls."