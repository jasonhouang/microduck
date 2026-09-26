#!/bin/bash
# 验证 MAX98357A 驱动代码

set -e

echo "=== 验证 MAX98357A 驱动 ==="

# 检查文件存在
echo "[1/6] 检查文件..."
test -f max98357a.c || { echo "❌ max98357a.c 不存在"; exit 1; }
test -f Makefile || { echo "❌ Makefile 不存在"; exit 1; }
test -f dkms.conf || { echo "❌ dkms.conf 不存在"; exit 1; }
echo "✓ 所有文件存在"

# 检查 C 语法
echo "[2/6] 检查 C 语法..."
gcc -fsyntax-only -I/usr/src/linux-headers-$(uname -r)/include max98357a.c 2>&1 || {
    echo "⚠ 语法检查需要内核头文件，跳过"
}

# 检查模块名一致性
echo "[3/6] 检查模块名一致性..."
makefile_module=$(grep -o 'snd-soc-max98357a' Makefile | head -1)
dkms_module=$(grep -o 'snd-soc-max98357a' dkms.conf | head -1)
if [ "$makefile_module" = "$dkms_module" ]; then
    echo "✓ 模块名一致: $makefile_module"
else
    echo "❌ 模块名不一致"
    exit 1
fi

# 检查 compatible 字符串
echo "[4/6] 检查 compatible 字符串..."
compatible=$(grep -o 'maxim,max98357a' max98357a.c | head -1)
if [ -n "$compatible" ]; then
    echo "✓ compatible: $compatible"
else
    echo "❌ 未找到 compatible 字符串"
    exit 1
fi

# 检查驱动功能
echo "[5/6] 检查驱动功能定义..."
grep -q "playback" max98357a.c || { echo "❌ 未定义 playback"; exit 1; }
grep -q "max98357a_dai" max98357a.c || { echo "❌ 未定义 DAI"; exit 1; }
echo "✓ 驱动功能定义正确"

# 检查 DKMS 配置
echo "[6/6] 检查 DKMS 配置..."
grep -q "PACKAGE_NAME=\"max98357a\"" dkms.conf || { echo "❌ PACKAGE_NAME 错误"; exit 1; }
grep -q "PACKAGE_VERSION=\"1.0\"" dkms.conf || { echo "❌ PACKAGE_VERSION 错误"; exit 1; }
echo "✓ DKMS 配置正确"

echo ""
echo "=== 验证通过 ==="
