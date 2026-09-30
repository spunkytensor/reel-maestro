#!/bin/sh
# Copyright 2026 Spunky Tensor
# SPDX-License-Identifier: Apache-2.0
set -eu

mkdir -p /sources /build /package /packages
curl -fsSL https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz -o /sources/ffmpeg-9.0.2.tar.xz
echo '8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e  /sources/ffmpeg-9.0.2.tar.xz' | sha256sum -c -
tar -xf /sources/ffmpeg-9.0.2.tar.xz -C /build
cd /build/ffmpeg-9.0.2
patch -p1 < /ffmpeg-loudnorm-silence.patch
cp /ffmpeg-loudnorm-silence.patch /sources/
./configure --prefix=/usr --disable-doc --disable-debug --disable-autodetect \
  --enable-gpl --enable-version3 --enable-libx264 --enable-libmp3lame \
  --enable-libass --enable-libfreetype --enable-libfontconfig \
  --enable-libharfbuzz --enable-openssl
make -j2
make install DESTDIR=/package
rm -rf /package/usr/include /package/usr/lib /package/usr/share/ffmpeg
cp COPYING.GPLv3 /sources/FFmpeg-COPYING.GPLv3
cp ffbuild/config.mak /sources/FFmpeg-config.mak

# APK v2 control + data streams. This is a local source build, NOT a Wolfi
# binary rebuild: use revision zero, not the distribution's patch revision.
# Installing a real package keeps FFmpeg in the OS inventory and CVE matching.
python3 - <<'PY'
import hashlib
import tarfile
from pathlib import Path

root = Path('/package')
with tarfile.open('/tmp/data.tar.gz', 'w:gz', format=tarfile.PAX_FORMAT) as archive:
    for path in sorted(root.rglob('*')):
        entry = archive.gettarinfo(str(path), str(path.relative_to(root)))
        if path.is_file():
            entry.pax_headers['APK-TOOLS.checksum.SHA1'] = hashlib.sha1(path.read_bytes()).hexdigest()
            with path.open('rb') as contents:
                archive.addfile(entry, contents)
        else:
            archive.addfile(entry)
PY
mkdir -p /control
cat >/control/.PKGINFO <<EOF
pkgname = ffmpeg-9.0
pkgver = 9.0.2-r0
pkgdesc = Reel Maestro FFmpeg source build with libass subtitle rendering
url = https://ffmpeg.org/
builddate = $(date +%s)
packager = Spunky Tensor
size = $(du -sk /package | awk '{print $1 * 1024}')
arch = $(apk --print-arch)
origin = ffmpeg-9.0
license = GPL-3.0-or-later
depend = libass
depend = freetype
depend = libfontconfig1
depend = fribidi
depend = harfbuzz
depend = lame-libs
depend = x264-libs
depend = libcrypto3
depend = libssl3
provides = ffmpeg=9.0.2-r0
datahash = $(sha256sum /tmp/data.tar.gz | cut -d ' ' -f1)
EOF
tar -C /control --format=ustar --blocking-factor=1 -cf /tmp/control.tar .PKGINFO
# APK control streams omit the two tar end-of-archive blocks.
head -c -1024 /tmp/control.tar | gzip -n >/tmp/control.tar.gz
cat /tmp/control.tar.gz /tmp/data.tar.gz >/packages/ffmpeg-9.0-9.0.2-r0.apk
