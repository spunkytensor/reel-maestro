#!/bin/sh
set -eu

run_dir=${1:-/data/out/docker-offline-fixture}
if [ "${run_dir#/data/out/}" = "$run_dir" ] || [ "$run_dir" = "/data/out/" ]; then
  echo "fixture output must be a child of /data/out" >&2
  exit 2
fi

rm -rf "$run_dir"
mkdir -p "$run_dir"
cat >"$run_dir/script.json" <<'JSON'
{
  "title": "Docker offline fixture",
  "narration": "",
  "music_prompt": "",
  "scenes": [
    {
      "line": "",
      "image_prompt": "offline fixture",
      "cast_ids": [],
      "location_id": "",
      "transition": "",
      "motion_prompt": ""
    }
  ]
}
JSON

ffmpeg -hide_banner -loglevel error -y \
  -f lavfi -i "color=c=0x14233b:s=1080x1920:d=1" \
  -frames:v 1 "$run_dir/scene-00.jpg"
cp "$run_dir/scene-00.jpg" "$run_dir/poster.jpg"
ffmpeg -hide_banner -loglevel error -y \
  -f lavfi -i "anullsrc=r=48000:cl=stereo" -t 2 \
  -codec:a libmp3lame "$run_dir/audio.mp3"
printf '[]\n' >"$run_dir/words.json"

reelmaestro --no-dotenv --from "$run_dir" \
  --video-provider local --no-captions --no-dissolve --no-grade \
  --no-embed-poster

test -s "$run_dir/reel.mp4"
ffprobe -v error -select_streams v:0 \
  -show_entries stream=codec_name,width,height \
  -of default=noprint_wrappers=1 "$run_dir/reel.mp4"
printf 'offline fixture rendered: %s\n' "$run_dir/reel.mp4"
