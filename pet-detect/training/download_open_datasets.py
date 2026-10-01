#!/usr/bin/env python3
"""
下载和准备开源音频数据集用于训练
- ESC-50: 环境声音（背景噪音）
- Kaggle Infant Cry: 婴儿哭声
"""

import os
import subprocess
import zipfile
import shutil
from pathlib import Path
import urllib.request

DATA_DIR = Path(__file__).parent.parent / "data"

def download_file(url, dest_path):
    """下载文件"""
    if dest_path.exists():
        print(f"已存在: {dest_path}")
        return True

    print(f"下载: {url}")
    print(f"到: {dest_path}")
    try:
        urllib.request.urlretrieve(url, dest_path)
        print("✓ 下载完成")
        return True
    except Exception as e:
        print(f"✗ 下载失败: {e}")
        return False

def extract_zip(zip_path, extract_to):
    """解压 ZIP 文件"""
    print(f"解压: {zip_path}")
    with zipfile.ZipFile(zip_path, 'r') as zip_ref:
        zip_ref.extractall(extract_to)
    print(f"✓ 解压到: {extract_to}")

def process_esc50():
    """下载和处理 ESC-50 数据集（环境声音作为背景噪音）"""
    print("\n=== ESC-50 环境声音数据集 ===")

    esc50_url = "https://github.com/karolpiczak/ESC-50/archive/master.zip"
    zip_path = DATA_DIR / "esc50.zip"
    extract_dir = DATA_DIR / "esc50_temp"

    # 下载
    if not download_file(esc50_url, zip_path):
        return False

    # 解压
    if extract_dir.exists():
        shutil.rmtree(extract_dir)
    extract_zip(zip_path, extract_dir)

    # 找到音频文件
    audio_dir = extract_dir / "ESC-50-master" / "audio"
    if not audio_dir.exists():
        print(f"✗ 找不到音频目录: {audio_dir}")
        return False

    # 创建 normal 目录
    normal_dir = DATA_DIR / "normal"
    normal_dir.mkdir(exist_ok=True)

    # 复制并重命名文件（只选择非人声、非动物的类别作为背景噪音）
    # ESC-50 类别：https://github.com/karolpiczak/ESC-50
    # 排除：人声（10-14, 20-24）、动物（0-4）
    # 选择：自然声音（5-9）、室内声（15-19）、城市声（25-49）
    bg_categories = list(range(5, 10)) + list(range(15, 20)) + list(range(25, 50))

    copied = 0
    for cat in bg_categories:
        cat_files = list(audio_dir.glob(f"{cat}-*.wav"))
        for f in cat_files:
            # 重命名为 esc50_<category>_<filename>.wav
            new_name = f"esc50_cat{cat}_{f.name}"
            dest = normal_dir / new_name
            if not dest.exists():
                shutil.copy2(f, dest)
                copied += 1

    print(f"✓ 复制了 {copied} 个背景噪音样本到 {normal_dir}")

    # 清理临时文件
    shutil.rmtree(extract_dir)
    zip_path.unlink()

    return True

def process_kaggle_infant_cry():
    """下载 Kaggle Infant Cry 数据集"""
    print("\n=== Kaggle Infant Cry 数据集 ===")

    # 检查是否安装了 kaggle CLI
    try:
        result = subprocess.run(["kaggle", "--version"], capture_output=True, text=True)
        if result.returncode != 0:
            print("✗ kaggle CLI 未安装")
            print("请安装: pip install kaggle")
            print("并配置 ~/.kaggle/kaggle.json")
            return False
    except FileNotFoundError:
        print("✗ kaggle CLI 未找到")
        print("请安装: pip install kaggle")
        return False

    # 下载数据集
    cry_dir = DATA_DIR / "cry"
    cry_dir.mkdir(exist_ok=True)

    print("下载 Kaggle Infant Cry Audio Corpus...")
    try:
        subprocess.run([
            "kaggle", "datasets", "download",
            "-d", "warcoder/infant-cry-audio-corpus",
            "--unzip",
            "-p", str(cry_dir)
        ], check=True)
        print("✓ 下载完成")

        # 重命名文件
        count = 0
        for f in cry_dir.glob("*.wav"):
            if not f.name.startswith("kaggle_"):
                new_name = f"kaggle_{f.name}"
                f.rename(f.parent / new_name)
                count += 1

        print(f"✓ 重命名了 {count} 个哭声样本")
        return True

    except subprocess.CalledProcessError as e:
        print(f"✗ 下载失败: {e}")
        return False

def main():
    print("=== 开源音频数据集下载和准备 ===\n")

    DATA_DIR.mkdir(exist_ok=True)

    # 1. ESC-50 背景噪音
    success_esc50 = process_esc50()

    # 2. Kaggle Infant Cry（需要 kaggle CLI）
    success_kaggle = process_kaggle_infant_cry()

    print("\n=== 总结 ===")
    print(f"ESC-50 背景噪音: {'✓ 成功' if success_esc50 else '✗ 失败'}")
    print(f"Kaggle 婴儿哭声: {'✓ 成功' if success_kaggle else '✗ 失败'}")

    if success_esc50 or success_kaggle:
        print("\n下一步:")
        print("1. 运行数据增强: python3 training/augment_data.py data")
        print("2. 重新训练模型: cd pet-detect && uv run training/train.py")

if __name__ == "__main__":
    main()
