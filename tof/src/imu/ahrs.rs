//! Madgwick AHRS 传感器融合。
//!
//! 基于 Sebastian Madgwick 的互补滤波算法，融合加速度计和陀螺仪数据，
//! 输出 body → world 的四元数姿态。
//!
//! 参考：Madgwick, S.O.H. "An efficient orientation filter for inertial and
//! inertial/magnetic sensor arrays" (2010).

/// Madgwick AHRS 滤波器。
///
/// 内部维护 body → world 的四元数（w-first），通过加速度计的重力方向
/// 修正陀螺仪积分漂移。
pub struct MadgwickAhrs {
    /// 收敛速率参数。较大 = 更快收敛到重力方向，但对噪声更敏感。
    beta: f64,
    /// 当前姿态四元数 [w, x, y, z]，body → world。
    q: [f64; 4],
}

impl MadgwickAhrs {
    /// 创建新的 AHRS 滤波器。
    ///
    /// `beta` 控制加速度计修正的权重：
    /// - 0.1 是推荐的起始值（与 BMI088 crate 一致）
    /// - 较大值（如 0.5）收敛更快但噪声更多
    /// - 较小值（如 0.01）更平滑但收敛慢
    pub fn new(beta: f64) -> Self {
        Self {
            beta,
            q: [1.0, 0.0, 0.0, 0.0], // 单位四元数：初始无旋转
        }
    }

    /// 更新姿态估计。
    ///
    /// # 参数
    /// - `accel`：加速度计读数 [ax, ay, az]，单位 g
    /// - `gyro`：陀螺仪读数 [gx, gy, gz]，单位 °/s
    /// - `dt`：两次更新之间的时间间隔，单位秒
    ///
    /// # 返回
    /// 当前姿态四元数 [w, x, y, z]
    pub fn update(&mut self, accel: [f64; 3], gyro: [f64; 3], dt: f64) -> [f64; 4] {
        let [q0, q1, q2, q3] = self.q;

        // 陀螺仪转换为弧度/秒
        let deg_to_rad = std::f64::consts::PI / 180.0;
        let gx = gyro[0] * deg_to_rad;
        let gy = gyro[1] * deg_to_rad;
        let gz = gyro[2] * deg_to_rad;

        // 四元数微分（陀螺仪积分）
        let q_dot = [
            0.5 * (-q1 * gx - q2 * gy - q3 * gz),
            0.5 * (q0 * gx + q2 * gz - q3 * gy),
            0.5 * (q0 * gy - q1 * gz + q3 * gx),
            0.5 * (q0 * gz + q1 * gy - q2 * gx),
        ];

        // 加速度计归一化
        let a_norm = (accel[0] * accel[0] + accel[1] * accel[1] + accel[2] * accel[2]).sqrt();
        if a_norm < 1e-10 {
            // 加速度计读数为零（自由落体或传感器故障），只用陀螺仪
            self.q = quat_normalize([
                q0 + q_dot[0] * dt,
                q1 + q_dot[1] * dt,
                q2 + q_dot[2] * dt,
                q3 + q_dot[3] * dt,
            ]);
            return self.q;
        }
        let ax = accel[0] / a_norm;
        let ay = accel[1] / a_norm;
        let az = accel[2] / a_norm;

        // 梯度下降：计算加速度计误差的梯度
        // f = 误差函数（预测重力方向 vs 测量重力方向）
        let f0 = 2.0 * (q1 * q3 - q0 * q2) - ax;
        let f1 = 2.0 * (q0 * q1 + q2 * q3) - ay;
        let f2 = 2.0 * (0.5 - q1 * q1 - q2 * q2) - az;

        // Jacobian 转置乘以梯度：∇f = J^T · f
        let step = [
            -2.0 * q2 * f0 + 2.0 * q1 * f1,
            2.0 * q3 * f0 + 2.0 * q0 * f1 - 4.0 * q1 * f2,
            -2.0 * q0 * f0 + 2.0 * q3 * f1 - 4.0 * q2 * f2,
            2.0 * q1 * f0 + 2.0 * q2 * f1,
        ];

        // 归一化梯度步长
        let step_norm = (step[0] * step[0] + step[1] * step[1] + step[2] * step[2] + step[3] * step[3]).sqrt();
        let step = if step_norm > 1e-10 {
            [
                step[0] / step_norm,
                step[1] / step_norm,
                step[2] / step_norm,
                step[3] / step_norm,
            ]
        } else {
            [0.0; 4]
        };

        // 融合：四元数微分 + 梯度修正
        self.q = quat_normalize([
            q0 + (q_dot[0] - self.beta * step[0]) * dt,
            q1 + (q_dot[1] - self.beta * step[1]) * dt,
            q2 + (q_dot[2] - self.beta * step[2]) * dt,
            q3 + (q_dot[3] - self.beta * step[3]) * dt,
        ]);

        self.q
    }

    /// 返回当前姿态四元数 [w, x, y, z]。
    pub fn quaternion(&self) -> [f64; 4] {
        self.q
    }

    /// 重置为单位四元数（无旋转）。
    pub fn reset(&mut self) {
        self.q = [1.0, 0.0, 0.0, 0.0];
    }
}

/// 四元数归一化。
fn quat_normalize(q: [f64; 4]) -> [f64; 4] {
    let norm = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if norm < 1e-15 {
        return [1.0, 0.0, 0.0, 0.0];
    }
    [q[0] / norm, q[1] / norm, q[2] / norm, q[3] / norm]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 静止状态下，加速度计读 1g 向上，陀螺仪读零，
    /// AHRS 应保持水平姿态。
    #[test]
    fn level_at_rest_stays_level() {
        let mut ahrs = MadgwickAhrs::new(0.1);
        // 静止：accel = [0, 0, 1] g, gyro = [0, 0, 0] °/s
        let accel = [0.0, 0.0, 1.0];
        let gyro = [0.0, 0.0, 0.0];

        // 运行 100 次（1 秒 @ 100 Hz）
        for _ in 0..100 {
            ahrs.update(accel, gyro, 0.01);
        }

        let q = ahrs.quaternion();
        // 应该接近单位四元数（无旋转）
        assert!((q[0] - 1.0).abs() < 0.1, "w should be near 1: {}", q[0]);
        assert!(q[1].abs() < 0.1, "x should be near 0: {}", q[1]);
        assert!(q[2].abs() < 0.1, "y should be near 0: {}", q[2]);
        assert!(q[3].abs() < 0.1, "z should be near 0: {}", q[3]);
    }

    /// 纯陀螺仪积分（无加速度计修正），90°/s 绕 Z 轴转 1 秒 = 90°。
    #[test]
    fn gyro_integration_tracks_rotation() {
        // 使用 beta=0 禁用加速度计修正
        let mut ahrs = MadgwickAhrs::new(0.0);
        let accel = [0.0, 0.0, 0.0]; // 零加速度（避免梯度修正）
        let gyro = [0.0, 0.0, 90.0]; // 90 °/s 绕 Z

        // 1 秒 @ 100 Hz
        for _ in 0..100 {
            ahrs.update(accel, gyro, 0.01);
        }

        let q = ahrs.quaternion();
        // 90° 绕 Z：q = [cos(45°), 0, 0, sin(45°)] ≈ [0.707, 0, 0, 0.707]
        let expected_w = (45.0_f64).to_radians().cos();
        let expected_z = (45.0_f64).to_radians().sin();
        assert!((q[0] - expected_w).abs() < 0.05, "w: {}", q[0]);
        assert!((q[3] - expected_z).abs() < 0.05, "z: {}", q[3]);
    }
}
