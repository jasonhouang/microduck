# Microduck 项目学习指南

## 简介

Microduck 是一个 25cm 高、800g 重的双足机器人，由强化学习策略驱动。这个项目文档将带你深入了解其硬件架构、软件设计、控制算法和工程实践。

## 学习路径

建议按以下顺序阅读文档：

### 1. [项目概览](00-项目概览.md)
- 项目定位和核心能力
- 硬件平台概览
- 软件架构概览
- 技术栈介绍

### 2. [硬件架构](01-硬件架构.md)
- Radxa Zero 3W 开发板
- Dynamixel XL330 舵机系统
- 单总线通信架构
- 传感器系统（IMU、ToF、摄像头）
- 电源管理

### 3. [软件架构](02-软件架构.md)
- 7 个守护进程的职责划分
- JSON-RPC IPC 通信机制
- 状态管理和配置系统
- 故障隔离策略

### 4. [控制循环](03-控制循环.md)
- 50Hz 实时控制循环
- Observation 向量构建（61 维）
- ONNX 策略推理
- 目标位置计算和低通滤波
- 技能优先级调度

### 5. [安全机制](04-安全机制.md)
- 摔倒检测与保护
- 关节限位和碰撞检测
- 电池电压监测
- 舵机过流/过热保护
- 通信超时保护
- 紧急停止机制

### 6. [更新系统](05-更新系统.md)
- OTA 更新包结构
- Ed25519 签名验证
- 原子更新和自动回滚
- 健康检查机制
- 版本管理策略

### 7. [部署流程](06-部署流程.md)
- 开发环境搭建
- 交叉编译配置
- 固件打包和签名
- 部署脚本使用
- CI/CD 集成

### 8. [设计亮点](07-设计亮点.md)
- 单总线多设备架构
- Rust 借用检查器实现安全保证
- 策略热切换机制
- 分层安全机制
- 模块化插件系统
- 实时性能优化
- 优雅降级策略
- 完善的日志与诊断系统

## 快速开始

### 环境要求
- Rust 1.89+ (stable)
- Linux 或 macOS 开发环境
- 交叉编译工具链（用于 ARM64 目标）

### 编译验证
```bash
# 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 克隆项目
git clone <repository-url>
cd microduck

# 编译检查
cargo check --workspace

# 运行测试
cargo test --workspace
```

### 关键代码位置
- **控制循环**: `robotd/src/control.rs`
- **安全机制**: `duck-control/src/safety.rs`
- **总线通信**: `duck-control/src/bus.rs`
- **IMU 解码**: `duck-control/src/imu.rs`
- **更新引擎**: `updater/src/engine.rs`
- **IPC 协议**: `duck-ipc-proto/src/lib.rs`

## 核心概念

### 1. 单总线架构
所有 15 个舵机和 1 个 IMU 共享一条 UART 总线（/dev/ttyS2），通过 Dynamixel Protocol v2 通信。这种设计简化了硬件布线，保证了传感器数据的同步性。

### 2. 50Hz 控制循环
每个控制周期（20ms）执行：
1. 读取传感器（~2ms）
2. 构建 Observation（~0.1ms）
3. ONNX 推理（~2ms）
4. 计算目标位置（~1ms）
5. 安全检查（~1ms）
6. 写入舵机（~2ms）

### 3. 安全优先
- 借用检查器强制 IO 独占访问
- 多层安全检查（关节限位、碰撞检测、电池保护）
- 摔倒检测和保护动作
- 自动回滚机制

### 4. 模块化设计
- 7 个独立守护进程
- JSON-RPC 统一接口
- 故障隔离，可独立重启
- 插件化扩展

## 技术栈

| 组件 | 技术 |
|------|------|
| 语言 | Rust 2024 edition |
| 异步运行时 | Tokio |
| 推理引擎 | ONNX Runtime (ort) |
| 通信协议 | JSON-RPC 2.0 over Unix socket |
| 硬件通信 | Dynamixel Protocol v2 (rustypot) |
| 媒体处理 | GStreamer |
| 远程访问 | WebRTC |

## 相关资源

- **产品页面**: https://pollen-robotics.com/microduck
- **Press Kit**: https://pollen-robotics.com/microduck/press-kit/
- **策略训练**: https://github.com/pollen-robotics/microduck_rl
- **架构文档**: ../design/architecture.md
- **贡献指南**: ../CONTRIBUTING.md

## 学习建议

1. **先理解整体架构**：从项目概览开始，建立全局视图
2. **深入核心模块**：控制循环和安全机制是核心，重点理解
3. **动手实践**：尝试编译、运行测试，理解代码结构
4. **阅读设计文档**：../design/ 目录下的文档解释了设计决策
5. **关注工程实践**：部署流程和设计亮点展示了工程最佳实践

## 常见问题

### Q: 为什么选择 Rust？
A: Rust 的借用检查器在编译期保证了内存安全和并发安全，这对于机器人系统至关重要。同时，Rust 的性能接近 C/C++，适合实时控制场景。

### Q: 为什么用单总线而不是多总线？
A: 单总线简化了硬件设计，更重要的是保证了所有传感器数据的同步性。在 50Hz 控制循环中，IMU 和舵机状态必须在同一时刻采集。

### Q: 为什么策略不直接使用摄像头和 ToF 数据？
A: 控制策略只使用本体感受（proprioception）数据，这是经过验证的 RL 训练方法。视觉数据用于更高层的行为决策，通过特征提取后传递给控制层。

### Q: 如何添加新的控制技能？
A: 在 microduck_rl 仓库中训练新的 ONNX 策略，然后在 robotd/src/control.rs 中添加技能调度逻辑。详见设计文档。

## 下一步

开始阅读 [00-项目概览.md](00-项目概览.md)，了解 Microduck 的全貌。
