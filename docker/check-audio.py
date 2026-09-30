# Copyright 2026 Spunky Tensor
# SPDX-License-Identifier: Apache-2.0
"""Verify short silent MP3 stays silent and audible audio is still normalized."""
import array
import math
import subprocess
import tempfile
from pathlib import Path


def ffmpeg(*args):
    return subprocess.run(
        ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", *args],
        check=True, stdout=subprocess.PIPE,
    ).stdout


with tempfile.TemporaryDirectory() as directory:
    for silent in (True, False):
        mp3 = str(Path(directory) / "input.mp3")
        source = "anullsrc=r=48000:cl=stereo" if silent else "sine=frequency=440:sample_rate=48000"
        ffmpeg("-f", "lavfi", "-i", source, "-t", "2", "-c:a", "libmp3lame", mp3)
        normalized = ffmpeg(
            "-i", mp3, "-af", "loudnorm=I=-14:TP=-1.5:LRA=11",
            "-ar", "44100", "-ac", "2", "-f", "f32le", "pipe:1",
        )
        samples = array.array("f", normalized)
        assert len(samples) > 44100, "missing audio output"
        assert all(math.isfinite(sample) for sample in samples), "non-finite audio"
        peak = max(abs(sample) for sample in samples)
        rms = math.sqrt(sum(sample * sample for sample in samples) / len(samples))
        if silent:
            assert peak < 1e-6, f"silence amplified to {peak}"
        else:
            # The source sine is ~0.088 RMS; bypassing normalization fails this.
            assert 0.15 < rms < 0.3, f"audible input was not normalized: RMS={rms}"
            assert peak <= 1, f"clipped normalized audio: peak={peak}"
        ffmpeg("-i", mp3, "-af", "loudnorm=I=-14:TP=-1.5:LRA=11",
               "-ar", "44100", "-ac", "2", "-c:a", "aac", "-b:a", "192k",
               "-f", "null", "-")
        print(f"{'silent' if silent else 'audible'} MP3: finite output, RMS={rms:.6f}")
