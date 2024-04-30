
mkdir fresh-images
cd fresh-images

mkdir 90
cd 90
curl https://get.pharo.org/64/90 | bash
zip -r Pharo64-90.zip *
cd ..

mkdir 100
cd 100
curl https://get.pharo.org/64/100 | bash
zip -r Pharo64-100.zip *
cd ..


mkdir 110
cd 110
curl https://get.pharo.org/64/110 | bash
zip -r Pharo64-110.zip *
cd ..


mkdir 120
cd 120
curl https://get.pharo.org/64/120 | bash
zip -r Pharo64-120.zip *
cd ..



mkdir 130
cd 130
curl https://get.pharo.org/64/130 | bash
zip -r Pharo64-130.zip *
cd ..


mkdir alpha
cd alpha
curl https://get.pharo.org/64/alpha | bash
zip -r Pharo64-alpha.zip *
cd ..

cd ..