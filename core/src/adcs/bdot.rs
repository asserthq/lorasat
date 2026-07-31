use nalgebra::Vector3;

pub struct BdotAlgorithm {
    prev_b: Vector3<f32>,
}

impl BdotAlgorithm {
    pub fn new() -> Self {
        Self {
            prev_b: Vector3::zeros(),
        }
    }

    pub fn calc_control(&mut self, b: Vector3<f32>) -> Vector3<f32> {
        if self.prev_b == Vector3::zeros() {
            self.prev_b = b;
            Vector3::zeros()
        } else {
            self.prev_b = b;
            let control = -self.b_dot_norm(b);
            control
        }
    }

    pub fn reset(&mut self) {
        self.prev_b = Vector3::zeros();
    }

    // fn b_dot(&self, field: Vector3<f32>, dt: Duration) -> Vector3<f32> {
    //     (field - self.prev_field) / dt.as_secs_f32()
    // }

    fn b_dot_norm(&self, b: Vector3<f32>) -> Vector3<f32> {
        let db = self.delta_b(b);
        if db.norm() > f32::EPSILON {
            db.normalize()
        } else {
            Vector3::zeros()
        }
    }

    fn delta_b(&self, b: Vector3<f32>) -> Vector3<f32> {
        b - self.prev_b
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
