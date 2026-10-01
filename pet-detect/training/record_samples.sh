#!/bin/bash
# 声音数据采集脚本
# 用法: ./record_samples.sh <类别> <数量>
# 示例: ./record_samples.sh quack 10

set -e

CATEGORY=${1:-quack}  # quack, cry, or normal
COUNT=${2:-10}
OUTPUT_DIR="../data/$CATEGORY"
SAMPLE_RATE=16000
DURATION=10  # 每个样本 10 秒

# 创建输出目录
mkdir -p "$OUTPUT_DIR"

echo "=== 声音数据采集 ==="
echo "类别: $CATEGORY"
echo "数量: $COUNT"
echo "每个样本: ${DURATION}秒"
echo "输出目录: $OUTPUT_DIR"
echo ""

# 检查已有多少样本
EXISTING=$(ls -1 "$OUTPUT_DIR"/*.wav 2>/dev/null | wc -l || echo 0)
echo "已有样本: $EXISTING"
echo ""

if [ "$EXISTING" -ge "$COUNT" ]; then
    echo "已有足够的 $CATEGORY 样本（$EXISTING >= $COUNT）"
    exit 0
fi

echo "准备录音..."
echo "每次录音前会有 3 秒倒计时"
echo "按 Ctrl+C 取消"
echo ""

# 开始录制
START_NUM=$((EXISTING + 1))
for i in $(seq $START_NUM $COUNT); do
    FILENAME=$(printf "%s_%03d.wav" "$CATEGORY" "$i")
    FILEPATH="$OUTPUT_DIR/$FILENAME"

    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "录制 $CATEGORY 样本 $i/$COUNT: $FILENAME"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # 倒计时
    for j in 3 2 1; do
        echo -n "$j... "
        sleep 1
    done
    echo "开始！"

    # 录音
    arecord -D plughw:i2saudio,0 -f S16_LE -r $SAMPLE_RATE -c 1 -d $DURATION "$FILEPATH"

    echo "✓ 已保存: $FILEPATH"
    echo ""

    # 间隔 2 秒
    if [ $i -lt $COUNT ]; then
        echo "准备下一个样本..."
        sleep 2
    fi
done

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✅ 完成！已录制 $((COUNT - EXISTING)) 个 $CATEGORY 样本"
echo "总计: $(ls -1 "$OUTPUT_DIR"/*.wav | wc -l) 个样本"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
