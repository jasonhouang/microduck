#!/usr/bin/env python3
"""
数据增强脚本：通过变调、变速、变音量生成更多训练样本
"""

import os
import sys
import numpy as np
import soundfile as sf
from pathlib import Path

def augment_audio(audio, sr, pitch_shift=0.0, speed=1.0, volume_db=0.0):
    """
    增强音频数据

    Args:
        audio: numpy array of audio samples
        sr: sample rate
        pitch_shift: 音调变化百分比 (e.g., 0.1 = +10%, -0.1 = -10%)
        speed: 速度变化 (e.g., 1.1 = 1.1x faster)
        volume_db: 音量变化 (dB)
    """
    # 变速
    if speed != 1.0:
        # 简单的重采样实现变速
        indices = np.round(np.arange(0, len(audio), speed)).astype(int)
        indices = indices[indices < len(audio)]
        audio = audio[indices]

    # 变调（通过重采样实现）
    if pitch_shift != 0.0:
        # 简单的变调：改变播放速度然后重采样回原长度
        factor = 1.0 + pitch_shift
        new_length = int(len(audio) / factor)
        indices = np.round(np.linspace(0, len(audio) - 1, new_length)).astype(int)
        audio = audio[indices]
        # 重采样回原长度
        x_old = np.linspace(0, 1, len(audio))
        x_new = np.linspace(0, 1, new_length)
        audio = np.interp(x_new, x_old, audio)

    # 变音量
    if volume_db != 0.0:
        factor = 10 ** (volume_db / 20.0)
        audio = audio * factor

    return audio

def augment_file(input_path, output_dir, prefix="aug"):
    """
    对单个文件进行多种增强
    """
    audio, sr = sf.read(input_path)
    base_name = Path(input_path).stem

    augmentations = [
        # (pitch_shift, speed, volume_db, suffix)
        (0.0, 1.0, 0.0, "orig"),      # 原始
        (0.1, 1.0, 0.0, "pitch_up"),  # 音调+10%
        (-0.1, 1.0, 0.0, "pitch_down"), # 音调-10%
        (0.0, 1.1, 0.0, "fast"),      # 速度+10%
        (0.0, 0.9, 0.0, "slow"),      # 速度-10%
        (0.0, 1.0, 6.0, "loud"),      # 音量+6dB
        (0.0, 1.0, -6.0, "quiet"),    # 音量-6dB
        (0.05, 1.05, 3.0, "combo1"),  # 组合1
        (-0.05, 0.95, -3.0, "combo2"), # 组合2
    ]

    created_files = []
    for pitch, speed, vol, suffix in augmentations:
        aug_audio = augment_audio(audio.copy(), sr, pitch, speed, vol)
        output_name = f"{prefix}_{base_name}_{suffix}.wav"
        output_path = Path(output_dir) / output_name
        sf.write(output_path, aug_audio, sr)
        created_files.append(output_path)

    return created_files

def main():
    if len(sys.argv) < 2:
        print("用法: python augment_data.py <data_directory>")
        print("示例: python augment_data.py ../data")
        sys.exit(1)

    data_dir = Path(sys.argv[1])

    if not data_dir.exists():
        print(f"错误: 目录不存在: {data_dir}")
        sys.exit(1)

    categories = ["quack", "cry", "normal"]

    for category in categories:
        category_dir = data_dir / category
        if not category_dir.exists():
            print(f"跳过: {category_dir} 不存在")
            continue

        print(f"\n=== 增强 {category} 数据 ===")

        # 查找所有原始样本（不以 aug_ 开头的）
        original_files = [f for f in category_dir.glob("*.wav") if not f.name.startswith("aug_")]

        print(f"找到 {len(original_files)} 个原始样本")

        if len(original_files) == 0:
            print(f"警告: {category} 没有原始样本")
            continue

        # 删除旧的增强文件
        old_aug_files = list(category_dir.glob("aug_*.wav"))
        for f in old_aug_files:
            f.unlink()
        print(f"已删除 {len(old_aug_files)} 个旧增强文件")

        # 增强每个原始文件
        total_created = 0
        for orig_file in original_files:
            created = augment_file(orig_file, category_dir, prefix="aug")
            total_created += len(created)

        print(f"✓ 创建了 {total_created} 个增强样本")

        # 统计总数
        all_files = list(category_dir.glob("*.wav"))
        print(f"  总计: {len(all_files)} 个样本")

if __name__ == "__main__":
    main()
