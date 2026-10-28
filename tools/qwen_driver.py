#!/usr/bin/env python3
"""torori's Qwen3-TTS driver (T4).

Boundary contract (owned by src/voice.rs): invoked as
`<python> qwen_driver.py --request <req.json> --out <dir>` where
req.json is {"text", "instruct", "seed", "takes", "temperature",
"top_p", "max_new_tokens"}. Writes take WAVs into <dir> and a
result.json {"model", "weights_sha256", "sample_rate",
"takes": [{"file", "seed"}]}.

The voice is DESIGNED by instruction text — no recorded person,
no reference audio, ever (brief §2 rights law). Model:
Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign (Apache-2.0), pinned by
the spike (D-084: Lane 2 admitted as the primary voice lane).
Weights sha256 = sha256 over the sorted per-file sha256 hex of
the snapshot's *.safetensors — recorded in every asset manifest.

Device: TORORI_QWEN_DEVICE (default "cuda:0"); the caller pins
the GPU via CUDA_VISIBLE_DEVICES under the good-neighbour
rules. Headless batch only — this script writes files and
metrics; it never plays anything (silence rule).
"""

import argparse
import hashlib
import json
import os
import sys

MODEL = "Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign"


def weights_sha256(snapshot_dir: str) -> str:
    per_file = []
    for name in sorted(os.listdir(snapshot_dir)):
        if name.endswith(".safetensors"):
            digest = hashlib.sha256()
            with open(os.path.join(snapshot_dir, name), "rb") as fh:
                for chunk in iter(lambda: fh.read(1 << 22), b""):
                    digest.update(chunk)
            per_file.append(f"{name}:{digest.hexdigest()}")
    joined = "\n".join(per_file).encode()
    return hashlib.sha256(joined).hexdigest()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--request", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    with open(args.request, encoding="utf-8") as fh:
        req = json.load(fh)
    os.makedirs(args.out, exist_ok=True)

    import soundfile as sf
    import torch
    from huggingface_hub import snapshot_download
    from qwen_tts import Qwen3TTSModel

    snapshot = snapshot_download(MODEL)
    sha = weights_sha256(snapshot)
    device = os.environ.get("TORORI_QWEN_DEVICE", "cuda:0")
    tts = Qwen3TTSModel.from_pretrained(
        MODEL, device_map=device, dtype=torch.bfloat16,
        attn_implementation="sdpa",
    )
    takes = []
    base_seed = int(req["seed"])
    for take in range(int(req["takes"])):
        seed = base_seed + take
        torch.manual_seed(seed)
        wavs, sr = tts.generate_voice_design(
            text=req["text"],
            language="English",
            instruct=req["instruct"],
            max_new_tokens=int(req.get("max_new_tokens", 1024)),
            temperature=float(req.get("temperature", 0.9)),
            top_p=float(req.get("top_p", 0.95)),
        )
        name = f"take{take}.wav"
        sf.write(os.path.join(args.out, name), wavs[0], sr)
        takes.append({"file": name, "seed": seed})
        print(f"GEN take{take} seed={seed} sr={sr} "
              f"dur={len(wavs[0]) / sr:.2f}s", flush=True)
    result = {
        "model": MODEL,
        "weights_sha256": sha,
        "sample_rate": sr,
        "takes": takes,
    }
    with open(os.path.join(args.out, "result.json"), "w", encoding="utf-8") as fh:
        json.dump(result, fh, indent=1)
    print("TORORI_QWEN_DRIVER_DONE", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
