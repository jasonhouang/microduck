#!/usr/bin/env python3
"""
处理下载的婴儿哭声数据集
- 转换为 16kHz 单声道 WAV
- 重命名为统一格式
- 复制到 data/cry 目录
"""

import os
import subprocess
from pathlib import Path

CRY_TEMP = Path("data/cry_temp")
CRY_DIR = Path("data/cry")

def convert_audio(input_path, output_path, sample_rate=16000):
    """使用 ffmpeg 转换音频为 16kHz 单声道 WAV"""
    cmd = [
        "ffmpeg", "-y", "-i", str(input_path),
        "-ar", str(sample_rate),
        "-ac", "1",
        "-sample_fmt", "s16",
        str(output_path)
    ]
    try:
        subprocess.run(cmd, capture_output=True, check=True)
        return True
    except subprocess.CalledProcessError as e:
        print(f"  ✗ 转换失败: {input_path}")
        print(f"    {e}")
        return False

def main():
    print("=== 处理婴儿哭声数据集 ===\n")

    # 查找所有 WAV 文件
    wav_files = list(CRY_TEMP.rglob("*.wav"))
    print(f"找到 {len(wav_files)} 个 WAV 文件")

    # 确保 cry 目录存在
    CRY_DIR.mkdir(exist_ok=True)

    # 获取当前 cry 目录中已有的文件数量
    existing_files = list(CRY_DIR.glob("kaggle_*.wav"))
    start_index = len(existing_files)
    print(f"已有 {len(existing_files)} 个 kaggle 样本")

    # 转换并复制文件
    converted = 0
    failed = 0

    for i, wav_file in enumerate(wav_files, start=start_index):
        output_name = f"kaggle_cry_{i:04d}.wav"
        output_path = CRY_DIR / output_name

        print(f"\r处理: {i+1}/{len(wav_files)} ({wav_file.name[:40]}...)", end="")

        if convert_audio(wav_file, output_path):
            converted += 1
        else:
            failed += 1

    print(f"\n\n✓ 完成！")
    print(f"  成功转换: {converted} 个")
    print(f"  失败: {failed} 个")
    print(f"  输出目录: {CRY_DIR}")

    # 统计 cry 目录总文件数
    total_files = list(CRY_DIR.glob("*.wav"))
    print(f"  cry 目录总计: {len(total_files)} 个文件")

if __name__ == "__main__":
    main()
