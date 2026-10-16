use core::time::Duration;
use nalgebra::Vector3;

pub struct BdotAlgorithm {
    // Предыдущее измерение магнитного поля.
    // Option нужен, чтобы отличать отсутствие измерения
    // от реального вектора, равного нулю.
    prev_b: Option<Vector3<f32>>,

    // Коэффициент управления.
    gain: f32,
}

impl BdotAlgorithm {
    pub fn new(gain: f32) -> Self {
        Self { prev_b: None, gain }
    }

    pub fn reset(&mut self) {
        // Следующий вызов будет первым измерением.
        self.prev_b = None;
    }

    pub fn calc_control(&mut self, b: Vector3<f32>, dt: Duration) -> Vector3<f32> {
        let dt_s = dt.as_secs_f32();
        if !dt_s.is_finite() || dt_s <= 0.0 {
            return Vector3::zeros();
        }

        // При первом измерении производную оценить нельзя.
        let Some(prev_b) = self.prev_b else {
            self.prev_b = Some(b);
            return Vector3::zeros();
        };

        // Оценка производной магнитного поля.
        let b_dot = (b - prev_b) / dt_s;

        // Сохраняем текущее измерение для следующего вызова.
        self.prev_b = Some(b);

        // Закон B-dot: требуемый магнитный дипольный момент.
        -self.gain * b_dot
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn zero_dipole_when_zero_field() {
//         let mut bdot_alg = BdotAlgorithm::new();
//         let result = bdot_alg.dipole_norm(Vector3::zeros(), Duration::from_secs(1));
//         assert_eq!(result, Vector3::zeros());
//     }

//     #[test]
//     fn zero_dipole_when_zero_dt() {
//         let mut bdot_alg = BdotAlgorithm::new();
//         let result = bdot_alg.dipole_norm(Vector3::zeros(), Duration::ZERO);
//         assert_eq!(result, Vector3::zeros());
//     }

//     #[test]
//     fn zero_dipole_when_static_field() {
//         let mut bdot_alg = BdotAlgorithm::new();
//         for _ in 0..3 {
//             let result = bdot_alg.dipole_norm(Vector3::new(1.0, 0.0, 0.0), Duration::from_secs(1));
//             assert_eq!(result, Vector3::zeros());
//         }
//     }
// }
