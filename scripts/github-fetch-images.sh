
mkdir fresh-images
cd fresh-images

mkdir 90
cd 90
curl https://get.pharo.org/64/90 | bash
cd ..

mkdir 100
cd 100
curl https://get.pharo.org/64/100 | bash
cd ..


mkdir 110
cd 110
curl https://get.pharo.org/64/110 | bash
cd ..


mkdir 120
cd 120
curl https://get.pharo.org/64/120 | bash
cd ..


mkdir alpha
cd alpha
curl https://get.pharo.org/64/alpha | bash
cd ..

zip -r fresh-images.zip *

cd ..