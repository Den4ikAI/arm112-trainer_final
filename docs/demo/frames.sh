#!/bin/bash
# Вынуть кадры ролика для проверки глазами:
#   docs/demo/frames.sh <ролик.mp4> <каталог> 40 120 300 …
set -euo pipefail
video="$1"; out="$2"; shift 2
mkdir -p "$out"
for t in "$@"; do
  ffmpeg -v error -y -ss "$t" -i "$video" -frames:v 1 -vf scale=960:-1 "$out/$t.jpg"
done
ls "$out"
