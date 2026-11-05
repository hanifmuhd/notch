#!/bin/bash
# Makes the four built-in bells (src/sounds/*.ogg) from sine partials with ffmpeg, so they carry
# no licence. Run from the repo root: bash tools/make_bells.sh
set -euo pipefail
out=src/sounds
mkdir -p "$out"
bell() { # name, expression, seconds
  ffmpeg -hide_banner -loglevel error -nostdin -y -f lavfi -i "aevalsrc='$2':s=44100:d=$3" \
    -af "afade=t=in:d=0.004,afade=t=out:st=$(echo "$3 - 0.25" | bc):d=0.25,loudnorm=I=-18:TP=-1.5" -ar 44100 -c:a libvorbis -q:a 4 "$out/$1.ogg"
}
# Chime: two notes, C6 then G5.
bell chime "0.5*sin(2*PI*1046.5*t)*exp(-3.2*t)+0.25*sin(2*PI*2093*t)*exp(-5*t)+0.4*sin(2*PI*784*(t-0.18))*exp(-3*(t-0.18))*gte(t,0.18)" 1.6
# Ding: one bright A5 with its octave.
bell ding "0.6*sin(2*PI*880*t)*exp(-2.6*t)+0.2*sin(2*PI*1760*t)*exp(-4.5*t)+0.08*sin(2*PI*2640*t)*exp(-7*t)" 1.4
# Soft: a low, slow E5 with a gentle start.
bell soft "(1-exp(-60*t))*(0.6*sin(2*PI*659.3*t)*exp(-1.8*t)+0.15*sin(2*PI*1318.5*t)*exp(-3*t))" 1.8
# Double: two quick E6 dings.
bell double "0.5*sin(2*PI*1318.5*t)*exp(-6*t)+0.15*sin(2*PI*2637*t)*exp(-9*t)+(0.5*sin(2*PI*1318.5*(t-0.17))*exp(-4*(t-0.17))+0.15*sin(2*PI*2637*(t-0.17))*exp(-7*(t-0.17)))*gte(t,0.17)" 1.3
