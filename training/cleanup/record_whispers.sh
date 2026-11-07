#!/bin/bash
# Records the 20 lines of whisper_lines.json, one at a time, for the whisper test
# (WHISPER.md). 16 kHz mono WAVs go to training/benchmark/private/whisper-real/<mic>/
# (git-ignored, never published).
#
#   ./record_whispers.sh laptop      # the MacBook microphone
#   ./record_whispers.sh airpods     # AirPods (or any other mic you pick)
#
# For each line: press Return, whisper it, press Return again. Type r and Return
# to redo the last line, q and Return to stop (finished lines are kept).
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
MIC=${1:?usage: record_whispers.sh laptop|airpods}
OUT="$HERE/../benchmark/private/whisper-real/$MIC"
mkdir -p "$OUT"

echo "Microphones:"
ffmpeg -hide_banner -f avfoundation -list_devices true -i "" 2>&1 | sed -n '/audio devices/,$p' | grep -E '^\[AVFoundation.*\] \[[0-9]+\]' | sed 's/^.*\] \[/  [/'
read -r -p "Number of the microphone to use: " DEV

python3 -c "import json,sys; [print(l['id'] + '\t' + l['say']) for l in json.load(open(sys.argv[1]))]" \
  "$HERE/whisper_lines.json" > "$OUT/.lines.tsv"

i=0
LINES=()
while IFS= read -r l; do LINES+=("$l"); done < "$OUT/.lines.tsv"
while [ $i -lt ${#LINES[@]} ]; do
  id=${LINES[$i]%%$'\t'*}
  say=${LINES[$i]#*$'\t'}
  echo
  echo "[$((i + 1))/${#LINES[@]}]  $say"
  read -r -p "Return to start (q to stop): " k
  [ "$k" = q ] && break
  ffmpeg -nostdin -hide_banner -loglevel error -y -f avfoundation -i ":$DEV" -ac 1 -ar 16000 -c:a pcm_s16le "$OUT/$id.wav" &
  pid=$!
  read -r -p "  recording... whisper it, then Return: " _
  kill -INT $pid 2>/dev/null
  wait $pid 2>/dev/null
  read -r -p "  saved $id.wav. Return for the next line, r to redo: " k
  [ "$k" = r ] && continue
  i=$((i + 1))
done
rm -f "$OUT/.lines.tsv"
echo
echo "Done: $(ls "$OUT"/wr*.wav 2>/dev/null | wc -l | tr -d ' ') lines in $OUT"
