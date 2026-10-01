#!/usr/bin/env python3
"""
Test the trained model against a known audio sample.
Uses the Rust pet-features binary so feature extraction exactly matches training/runtime.
"""

import subprocess
import sys
from pathlib import Path

import numpy as np
import onnxruntime as ort

REPO = Path(__file__).resolve().parent.parent
# Must match src/lib.rs / train.py.
N_MELS = 40
WINDOW_FRAMES = 100
BLOCK_BYTES = N_MELS * WINDOW_FRAMES * 4  # f32 LE


def extract_features(wav: Path) -> np.ndarray:
    """Run pet-features binary, return first window as [1, 1, N_MELS, WINDOW_FRAMES]."""
    features_bin = REPO.parent / "target" / "release" / "pet-features"
    if not features_bin.exists():
        raise SystemExit(f"missing {features_bin} — run `cargo build --release -p pet-detect --bin pet-features`")
    res = subprocess.run([str(features_bin), str(wav)], capture_output=True, check=True)
    raw = res.stdout
    if len(raw) % BLOCK_BYTES != 0:
        raise RuntimeError(f"{wav}: feature byte count {len(raw)} not divisible by {BLOCK_BYTES}")
    n = len(raw) // BLOCK_BYTES
    arr = np.frombuffer(raw, dtype=np.float32).reshape(n, N_MELS, WINDOW_FRAMES).copy()
    print(f"  {n} windows extracted")
    # Return all windows for per-window analysis
    return arr


def main():
    if len(sys.argv) < 2:
        print("Usage: python3 test_model.py <audio_file>")
        sys.exit(1)

    audio_path = Path(sys.argv[1])
    if not audio_path.exists():
        print(f"File not found: {audio_path}")
        sys.exit(1)

    print(f"Testing: {audio_path.name}")

    # Extract features using Rust binary
    windows = extract_features(audio_path)

    # Load model
    model_path = REPO / "models" / "pet_detect.onnx"
    if not model_path.exists():
        print(f"Model not found: {model_path}")
        sys.exit(1)

    session = ort.InferenceSession(str(model_path))
    input_name = session.get_inputs()[0].name

    # Test each window
    print(f"\nPer-window results:")
    class_names = ["normal", "quack", "cry"]
    max_cry = 0.0
    max_quack = 0.0

    for i, window in enumerate(windows):
        input_data = window.reshape(1, 1, N_MELS, WINDOW_FRAMES).astype(np.float32)
        outputs = session.run(None, {input_name: input_data})
        probs = outputs[0][0]
        pred = class_names[np.argmax(probs)]
        max_cry = max(max_cry, probs[2])
        max_quack = max(max_quack, probs[1])
        print(f"  window {i:2d}: normal={probs[0]:.4f} quack={probs[1]:.4f} cry={probs[2]:.4f} -> {pred}")

    print(f"\nSummary:")
    print(f"  Max cry confidence:  {max_cry*100:.1f}%")
    print(f"  Max quack confidence: {max_quack*100:.1f}%")
    print(f"  Threshold: 99%")


if __name__ == "__main__":
    main()
