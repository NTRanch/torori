#!/usr/bin/env python3
"""Qwen3-ASR driver for torori's Tier 1 QC (PLAN SS4): transcribe
one existing WAV and print one JSON object on stdout:

    {"model": "Qwen/Qwen3-ASR-0.6B", "transcript": "..."}

Contract (owned by src/asr.rs): invoked as

    <venv-python> qwen_asr_driver.py --audio <wav>

Analysis only -- this driver transcribes delivered files; it
has no synthesis path. Headless: it reads files and prints
metrics; it never plays anything (the estate audio-silence
rule). Model cache: HF_HOME (primo: ~/.local/share/models/hf).

Device: TORORI_ASR_DEVICE, default "cuda:0". On primo the
good-neighbour rules prefer the A6000 eGPU: launch with
CUDA_VISIBLE_DEVICES=1 so cuda:0 is the A6000.
"""

import argparse
import json
import os
import sys


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--audio", required=True, help="path to an existing WAV")
    args = ap.parse_args()

    if not os.path.isfile(args.audio):
        print(f"audio file not found: {args.audio}", file=sys.stderr)
        return 2

    from qwen_asr import Qwen3ASRModel

    device = os.environ.get("TORORI_ASR_DEVICE", "cuda:0")
    model = Qwen3ASRModel.from_pretrained(
        "Qwen/Qwen3-ASR-0.6B",
        dtype="auto",
        device_map=device,
    )
    results = model.transcribe(audio=args.audio, language="English")
    transcript = results[0].text if results else ""
    print(json.dumps({"model": "Qwen/Qwen3-ASR-0.6B", "transcript": transcript}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
