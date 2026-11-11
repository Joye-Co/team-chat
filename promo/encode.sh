#!/bin/sh
# encode.sh <out.mp4> [crf]
# frames/%06d.jpg (4 samples per frame) + promo.wav -> mp4. Averaging each frame's samples gives the motion blur.
ffmpeg -loglevel error -y -framerate 240 -i frames/%06d.jpg -i promo.wav \
  -filter_complex "[0:v]format=gbrp,tmix=frames=4:weights='1 1 1 1',select='eq(mod(n\,4)\,3)',setpts=N/60/TB,format=yuv420p[v]" \
  -map "[v]" -map 1:a -r 60 -c:v libx264 -preset slow -crf "${2:-17}" \
  -colorspace bt709 -color_primaries bt709 -color_trc bt709 -c:a aac -b:a 256k -movflags +faststart -shortest "$1"
